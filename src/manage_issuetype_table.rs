use std::collections::HashSet;
use sqlx::{FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;
use crate::utils::get_inputs_not_in_db;

#[derive(FromRow, Debug, Eq, PartialEq, Hash)]
pub(crate) struct IssueType {
  jira_id: u32,
  name: String,
  description: String,
}

async fn get_issue_types_from_database(config: &Config, db_conn: &Pool<Sqlite>) -> Vec<IssueType> {
  let query_str =
    "SELECT  jira_id, name, description
         FROM IssueType;";

  let rows = sqlx::query_as::<_, IssueType>(query_str)
    .fetch_all(db_conn)
    .await;

  match rows {
    // todo(perf) simply rename fields in ProjectShortData to avoid the need of this conversion
    Ok(data) => { data }
    Err(e) => {
      eprintln!("Error occurred while trying to get issue types from local database: {e}");
      Vec::new()
    }
  }
}


async fn get_issue_types_from_server(config: &Config) -> Result<Vec<IssueType>, String>{
  let query = "/rest/api/2/issuetype";
  let json_data = get_json_from_url(config, query).await;
  let Ok(json_data) = json_data else {
    return Err(format!("Error: failed to get list of issue types from server.\n{e}", e=json_data.err().unwrap().to_string()));
  };

  let Some(json_array) = json_data.as_array() else {
    return Err(format!("Error: Returned data is unexpected. Expecting a json object, got [{e}]", e = json_data.to_string()));
  };

  let res = json_array
    .iter()
    .filter_map(|x| {
      let name = get_str_for_key(&x, "name")?;
      let description = get_str_for_key(&x, "description")?;
      let jira_id = get_str_for_key(&x, "id")?;
      let jira_id = match jira_id.parse::<u32>() {
        Ok(k) => { k }
        Err(e) => {
          eprintln!("Error: failed to parse a jira_id as integer. id was [{jira_id}]. Error was {e}. Ignoring it");
          return None;
        }
      };

      Some(IssueType {
        jira_id,
        name: name.to_string(),
        description: description.to_string(),
      })
    })
    .collect::<Vec<_>>();

//    dbg!(&res);
  Ok(res)
}


fn get_issue_types_not_in_db<'a, 'b>(issue_types: &'a Vec<IssueType>, issue_types_in_db: &'b Vec<IssueType>)
                                     -> Vec<&'a IssueType>
  where 'b: 'a
{
  get_inputs_not_in_db(issue_types.as_slice(), issue_types_in_db.as_slice())
}

async fn insert_issuetypes_to_database(db_conn: &mut Pool<Sqlite>, issuetypes_to_insert: Vec<&IssueType>) {
  if issuetypes_to_insert.is_empty() {
    eprintln!("No new issue type found");
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
    "INSERT INTO IssueType (jira_id, name, description) VALUES
                (?, ?, ?)
            ON CONFLICT DO
            UPDATE SET name = excluded.name, description = excluded.description";

  for IssueType { jira_id, name, description } in issuetypes_to_insert {
    let res = sqlx::query(query_str)
      .bind(jira_id)
      .bind(name)
      .bind(description)
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
    eprintln!("Error occurred while updating the database with issue types")
  } else {
    eprintln!("updated issue types in database: {row_affected} rows were updated")
  }
}


pub(crate)
async fn update_issue_types_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  let issue_types_to_insert = get_issue_types_from_server(&config).await;
  let Ok(issue_types_to_insert) = issue_types_to_insert else {
    eprintln!("Error: failed to get issue types from server: Err=[{e}]", e = issue_types_to_insert.err().unwrap());
    return;
  };
  let issue_types_in_db = get_issue_types_from_database(&config, &db_conn).await;
  let issue_types_to_insert = get_issue_types_not_in_db(&issue_types_to_insert, &issue_types_in_db);

  insert_issuetypes_to_database(db_conn, issue_types_to_insert).await;
}

