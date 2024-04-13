use std::collections::HashSet;
use sqlx::{FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;
use crate::manage_project_table::ProjectShortData;
use crate::utils::get_inputs_not_in_db;

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
    // todo(perf) simply rename fields in ProjectShortData to avoid the need of this conversion
    Ok(data) => { data }
    Err(e) => {
      eprintln!("Error occurred while trying to get projects from local database: {e}");
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
        return None;
      };
//      dbg!(x);
      let Some(jira_id) = x.get("id") else {
        return None;
      };
      let Some(jira_id) = jira_id.as_str() else {
         return None;
      };
//      dbg!(jira_id);
      let Some(human_name) = x.get("name") else {
        return None;
      };
      let Some(human_name) = human_name.as_str() else {
        return None;
      };
//      dbg!(human_name);
      let Some(key) = x.get("key") else {
        return None;
      };
      let Some(key) = key.as_str() else {
        return None;
      };
//      dbg!(key);
      let Some(schema) = x.get("schema") else {
        return None;
      };
      let schema = schema.to_string();
      let Some(custom) = x.get("custom") else {
        return None;
      };
//      dbg!(custom);
      let Some(is_custom) = custom.as_bool() else {
        return None;
      };
      Some(Field {
        jira_id: jira_id.to_string(),
        key: key.to_string(),
        human_name: human_name.to_string(),
        schema,
        is_custom,
      })
    })
    .collect::<Vec<_>>();
   
  Ok(res)
}


fn get_fields_not_in_db<'a, 'b>(fields: &'a Vec<Field>, fields_in_db: &'b Vec<Field>)
                                    -> Vec<&'a Field>
  where 'b: 'a
{
  get_inputs_not_in_db(fields.as_slice(), fields_in_db.as_slice())
}

async fn insert_fields_to_database(db_conn: &mut Pool<Sqlite>, fields_to_insert: Vec<&Field>) {
  if fields_to_insert.is_empty() {
    println!("No new field found");
    return;
  }

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
    println!("Error occurred while updating the database with Link types")
  } else {
    println!("updated Link types in database: {row_affected} rows were updated")
  }
}


pub(crate)
async fn update_fields_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  let fields_to_insert = get_fields_from_server(&config).await;
  let Ok(fields_to_insert) = fields_to_insert else {
    println!("Error: failed to get link types from server: Err=[{e}]", e = fields_to_insert.err().unwrap());
    return;
  };
//  dbg!(&fields_to_insert);
  let fields_in_db = get_fields_from_database(&config, &db_conn).await;
  let fields_to_insert = get_fields_not_in_db(&fields_to_insert, &fields_in_db);
//  dbg!(&fields_in_db);
//  dbg!(&fields_to_insert);

  insert_fields_to_database(db_conn, fields_to_insert).await;
}

