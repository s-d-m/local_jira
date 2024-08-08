use std::collections::HashSet;
use sqlx::{FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;
use crate::utils::get_inputs_in_remote_not_in_db;

#[derive(FromRow, Debug, Eq, PartialEq, Hash)]
pub(crate) struct IssueLinkType {
  jira_id: u32,
  name: String,
  outward_name: String,
  inward_name: String,
}

async fn get_link_types_from_database(db_conn: &Pool<Sqlite>) -> Vec<IssueLinkType> {
  let query_str =
    "SELECT  jira_id, name, outward_name, inward_name
         FROM IssueLinkType;";

  let rows = sqlx::query_as::<_, IssueLinkType>(query_str)
    .fetch_all(db_conn)
    .await;

  match rows {
    // todo(perf) simply rename fields in ProjectShortData to avoid the need of this conversion
    Ok(data) => { data }
    Err(e) => {
      eprintln!("Error occurred while trying to get link types from local database: {e}");
      Vec::new()
    }
  }
}


async fn get_issue_link_types_from_server(config: &Config) -> Result<Vec<IssueLinkType>, String>{
  let query = "/rest/api/2/issueLinkType";
  let json_data = get_json_from_url(config, query).await;
  let Ok(json_data) = json_data else {
    return Err(format!("Error: failed to get list of link types from server.\n{e}", e=json_data.err().unwrap().to_string()));
  };

  let Some(json_array) = json_data.as_object() else {
    return Err(format!("Error: Returned data is unexpected. Expecting a json object, got [{e}]", e = json_data.to_string()));
  };

  let Some(json_array) = json_array.get("issueLinkTypes") else {
    return Err(format!("Error: Returned data is unexpected. Expecting a key named issueLinkTypes, got [{e}]", e = json_data.to_string()));
  };

  let Some(json_array) = json_array.as_array() else {
    return Err(format!("Error: Returned data is unexpected. Expecting a json array, got [{e}]", e = json_data.to_string()));
  };

  let res = json_array
    .iter()
    .filter_map(|x| {
      let name = get_str_for_key(&x, "name")?;
      let outward_name = get_str_for_key(&x, "outward")?;
      let inward_name = get_str_for_key(&x, "inward")?;
      let jira_id = get_str_for_key(&x, "id")?;
      let jira_id = match jira_id.parse::<u32>() {
        Ok(k) => { k }
        Err(e) => {
          eprintln!("Error: failed to parse a jira_id as integer. id was [{jira_id}]. Error was {e}. Ignoring it");
          return None;
        }
      };

      Some(IssueLinkType {
        jira_id,
        name: name.to_string(),
        outward_name: outward_name.to_string(),
        inward_name: inward_name.to_string()
      })
    })
    .collect::<Vec<_>>();

//    dbg!(&res);
  Ok(res)
}


fn get_link_types_not_in_db<'a, 'b>(link_types: &'a Vec<IssueLinkType>, link_types_in_db: &'b Vec<IssueLinkType>)
                                    -> Vec<&'a IssueLinkType>
  where 'b: 'a
{
  get_inputs_in_remote_not_in_db(link_types.as_slice(), link_types_in_db.as_slice())
}

async fn insert_issue_link_types_to_database(db_conn: &mut Pool<Sqlite>, issue_link_types_to_insert: Vec<&IssueLinkType>) {
  if issue_link_types_to_insert.is_empty() {
    eprintln!("No new link type found");
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

  // todo(perf): add detection of what is already in db and do some filter out. Here we happily
  // overwrite data with the exact same ones, thus taking the write lock on the
  // database for longer than necessary.
  // Plus it means the logs aren't that useful to troubleshoot how much data changed
  // in the database. Seeing messages saying
  // 'updated Issue fields in database: 58 rows were updated'
  // means there has been at most 58 changes. Chances are there are actually been
  // none since the last update.
  let query_str =
    "INSERT INTO IssueLinkType (jira_id, name, outward_name, inward_name) VALUES
                (?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET name = excluded.name, inward_name = excluded.inward_name, outward_name = excluded.outward_name";

  for IssueLinkType { jira_id, name, outward_name, inward_name } in issue_link_types_to_insert {
    let res = sqlx::query(query_str)
      .bind(jira_id)
      .bind(name)
      .bind(outward_name)
      .bind(inward_name)
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
    eprintln!("updated Link types in database: {row_affected} rows were updated")
  }
}


pub(crate)
async fn update_issue_link_types_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  let link_types_to_insert = get_issue_link_types_from_server(&config).await;
  let Ok(link_types_to_insert) = link_types_to_insert else {
    eprintln!("Error: failed to get link types from server: Err=[{e}]", e = link_types_to_insert.err().unwrap());
    return;
  };
  let link_types_in_db = get_link_types_from_database(&db_conn).await;
  let links_to_insert = get_link_types_not_in_db(&link_types_to_insert, &link_types_in_db);


  insert_issue_link_types_to_database(db_conn, links_to_insert).await;
}

