use std::collections::HashSet;
use serde_json::Value;
use sqlx::{FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;
use crate::utils::{get_inputs_in_db_not_in_remote, get_inputs_in_remote_not_in_db};

#[derive(FromRow, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Field {
  jira_id: String,
  key: String,
  human_name: String,
  schema: String, // json
  is_custom: bool,
}

async fn get_fields_from_database(config: &Config, db_conn: &Pool<Sqlite>) -> Vec<Field> {
  let query_str =
    "SELECT  jira_id, key, human_name, schema, is_custom
         FROM Field;";

  let rows = sqlx::query_as::<_, Field>(query_str)
    .fetch_all(db_conn)
    .await;

  match rows {
    Ok(data) => { data }
    Err(e) => {
      eprintln!("Error occurred while trying to get fields from local database: {e}");
      Vec::new()
    }
  }
}


async fn get_fields_from_server(config: &Config) -> Result<Vec<Field>, String>{
  let query = "/rest/api/2/field";
  let json_data = get_json_from_url(config, query).await;
  let Ok(json_data) = json_data else {
    return Err(format!("Error: failed to get list of fields from server.\n{e}", e=json_data.err().unwrap().to_string()));
  };

  let Some(json_array) = json_data.as_array() else {
    return Err(format!("Error: Returned data is unexpected. Expecting a json object, got [{e}]", e = json_data.to_string()));
  };

  let res = json_array
    .into_iter()
    .filter_map(|x| {
//      dbg!(x);
      let Some(x) = x.as_object() else {
        eprintln!("Unexpected data. data should be a json object. data is {x} instead", x=x.to_string());
        return None;
      };
//      dbg!(x);
      let Some(jira_id) = x.get("id") else {
        eprintln!("Unexpected data. 'id' field is missing from data. data is {x:?} instead");
        return None;
      };
      let Some(jira_id) = jira_id.as_str() else {
        eprintln!("Unexpected data. 'jira_id' should be a string. it is {x} instead", x=jira_id.to_string());
        return None;
      };
//      dbg!(jira_id);
      let Some(human_name) = x.get("name") else {
        eprintln!("Unexpected data. 'name' field is missing from data. data is {x:?} instead");
        return None;
      };
      let Some(human_name) = human_name.as_str() else {
        eprintln!("Unexpected data. 'name' field should be a string. it is {x} instead", x=human_name.to_string());
        return None;
      };
//      dbg!(human_name);
      let Some(key) = x.get("key") else {
        eprintln!("Unexpected data. 'key' field is missing from data. data is {x:?} instead");
        return None;
      };
      let Some(key) = key.as_str() else {
        eprintln!("Unexpected data. 'key' field should be string. key is {x} instead", x=key.to_string());
        return None;
      };
//      dbg!(key);
      let Some(schema) = x.get("schema") else {
        eprintln!("Unexpected data. 'schema' field is missing from data. data is {x:?} instead");
        return None;
      };
      let Some(custom) = x.get("custom") else {
        eprintln!("Unexpected data. 'custom' field is missing from data. data is {x:?} instead");
        return None;
      };
//      dbg!(custom);
      let Some(is_custom) = custom.as_bool() else {
        eprintln!("Unexpected data. 'custom' field should be a boolean. It is {custom:?} instead");
        return None;
      };
      Some(Field {
        jira_id: jira_id.to_string(),
        key: key.to_string(),
        human_name: human_name.to_string(),
        schema: schema.to_string(),
        is_custom,
      })
    })
    .collect::<Vec<_>>();
   
  Ok(res)
}


fn get_fields_in_remote_not_in_db<'a, 'b>(fields: &'a Vec<Field>, fields_in_db: &'b Vec<Field>)
                                          -> Vec<&'a Field>
  where 'b: 'a
{
  get_inputs_in_remote_not_in_db(fields.as_slice(), fields_in_db.as_slice())
}

fn get_fields_in_db_not_in_remote<'a, 'b>(fields_in_remote: &'a Vec<Field>, fields_in_db: &'b Vec<Field>)
                                          -> Vec<&'a Field>
where 'b: 'a
{
  get_inputs_in_db_not_in_remote(fields_in_remote.as_slice(), fields_in_db.as_slice())
}

pub(crate)
async fn update_fields_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  let fields_in_remote = get_fields_from_server(&config).await;
  let fields_in_remote = match fields_in_remote {
    Ok(v) => {v}
    Err(e) => {
      eprintln!("Error: failed to get link types from server: Err=[{e}]");
      return;
    }
  };
//  dbg!(&fields_to_insert);
  let fields_in_db = get_fields_from_database(&config, &db_conn).await;
  let fields_to_insert = get_fields_in_remote_not_in_db(&fields_in_remote, &fields_in_db);
  let fields_to_remove = get_fields_in_db_not_in_remote(&fields_in_remote, &fields_in_db);
//  dbg!(&fields_in_db);
//  dbg!(&fields_to_insert);

  match fields_to_remove.is_empty() {
    true => { eprintln!("No fields in local db have been removed in remote db")}
    false => {
      let query_str = "DELETE FROM Field
                      WHERE jira_id = ?;";

      let mut has_error = false;
      let mut row_affected = 0;
      let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

      for Field{ jira_id, key, human_name, schema, is_custom } in fields_to_remove
      {
        let res = sqlx::query(query_str)
          .bind(jira_id)
          .execute(&mut *tx)
          .await;

        match res {
          Ok(e) => row_affected += e.rows_affected(),
          Err(e) => {
            has_error = true;
            eprintln!("Error when removing field with (jira_id {jira_id}, key: {key}, human_name: {human_name}), is_custom: {is_custom}. Err: {e}");
          }
        }
      }

      tx.commit().await.unwrap();

      if has_error {
        eprintln!("Error occurred while updating the database with issue fields")
      } else {
        eprintln!("updated Issue fields in database: {row_affected} rows were deleted")
      }
    }
  }

  match fields_to_insert.is_empty() {
    true => { eprintln!("No new field in remote found"); }
    false => {
      let mut has_error = false;
      let mut row_affected = 0;
      let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

      // todo(perf): these insert are likely very inefficient since we insert
      // one element at a time instead of doing bulk insert.
      // check https://stackoverflow.com/questions/65789938/rusqlite-insert-multiple-rows
      // and https://www.sqlite.org/c3ref/c_limit_attached.html#sqlitelimitvariablenumber
      // for the SQLITE_LIMIT_VARIABLE_NUMBER maximum number of parameters that can be
      // passed in a query.
      // splitting an iterator in chunks would come in handy here.
      let query_str =
        "INSERT INTO Field (jira_id, key, human_name, schema, is_custom) VALUES
                (?, ?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET human_name = excluded.human_name,
                       schema = excluded.schema,
                       is_custom = excluded.is_custom,
                       key = excluded.key";

      for Field { jira_id, key, human_name, schema, is_custom } in fields_to_insert {
        let res = sqlx::query(query_str)
          .bind(jira_id)
          .bind(key)
          .bind(human_name)
          .bind(schema)
          .bind(is_custom)
          .execute(&mut *tx)
          .await;
        match res {
          Ok(e) => { row_affected += e.rows_affected() }
          Err(e) => {
            has_error = true;
            eprintln!("Error: {e}")
          }
        }
      }

      tx.commit().await.unwrap();

      if has_error {
        eprintln!("Error occurred while updating the database with Link types")
      } else {
        eprintln!("updated fields types in database: {row_affected} rows were inserted")
      }
    }
  }
}

