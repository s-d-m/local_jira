use std::collections::HashSet;
use std::hash::Hash;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, Pool, Sqlite};
use sqlx::types::{Json, JsonValue};
use tokio::net::tcp::ReuniteError;
use crate::get_config::Config;
use crate::get_project_tasks_from_server::get_project_tasks_from_server;


#[derive(FromRow, Hash, PartialEq, Eq, Debug)]
struct Issue {
  jira_id: u32,
  key: String
}

fn get_issues_from_json(json_value: Result<Value, String>) -> Result<Vec<Issue>, String> {
  let Ok(json_data) = json_value else {
    return Err(format!("Error: couldn't extract issues from json: {e}", e = json_value.clone().err().unwrap()));
  };

  let Some(v) = json_data.get("issues") else {
    return Err(String::from("No field named 'issues' in the json"));
  };

  let Some(v) = v.as_array() else {
    return Err(String::from("Error: the fields named 'issues' isn't a json array"));
  };

  let res = v
    .into_iter()
    .filter_map(|x| x.as_object())
    .filter_map(|x| {
      let Some(key) = x.get("key") else {
        return None;
      };
      let Some(jira_id) = x.get("id") else {
        return None;
      };
      let Some(jira_id) = jira_id.as_str() else {
        return None;
      };
      let Ok(jira_id) = jira_id.parse::<u32>() else {
        return None;
      };
      Some(Issue { jira_id, key: key.to_string() })
    })
    .collect::<Vec<_>>();

  Ok(res)
}

#[derive(FromRow, Hash, PartialEq, Eq, Debug)]
pub(crate) struct IssueType {
  jira_id: u32,
  name: String,
  description: String,
}

async fn get_issues_from_db(db_conn: &Pool<Sqlite>) -> Result<Vec<Issue>, String> {
  let query_str =
    "SELECT  jira_id, key
     FROM Issue;";

  let rows = sqlx::query_as::<_, Issue>(query_str)
    .fetch_all(db_conn)
    .await;

  rows.map_err(|e| {
    format!("Error occurred while trying to get issues from local database: {e}", e = e.to_string())
  })
}


#[derive(FromRow, Debug)]
pub(crate) struct fields_in_db {
  jira_field_name: String,
  human_name: String,
}


async fn update_issues_in_db(issues_to_insert: &Vec<Issue>, db_conn: &mut Pool<Sqlite>) {
  let issues_in_db = get_issues_from_db(&db_conn).await;
  let issues_in_db = match issues_in_db {
    Ok(v) => v,
    Err(e) => {
      println!("Error occurred: {e}");
      Vec::new()
    }
  };
  
  let hashed_issues_in_db = issues_in_db.iter().collect::<HashSet<&Issue>>();
  let issues_to_insert = issues_to_insert
    .iter()
    .filter(|x| !hashed_issues_in_db.contains(x))
    .collect::<Vec<_>>();
  
  if issues_to_insert.is_empty() {
    println!("No new issue found");
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
    "INSERT INTO Issue (jira_id, key) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET key = excluded.key";

  for Issue { jira_id, key: key } in issues_to_insert {
    let res = sqlx::query(query_str)
      .bind(jira_id)
      .bind(key)
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
    println!("Error occurred while updating the database with Issue types")
  } else {
    println!("updated Issues in database: {row_affected} rows were updated")
  }
}

pub(crate) async fn update_interesting_projects_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  for project_key in config.interesting_projects() {
    let json_tickets = get_project_tasks_from_server(project_key, &config).await;

    let issues = get_issues_from_json(json_tickets);
    if let Ok(issues) = issues {
      update_issues_in_db(&issues, db_conn).await;
    }
  }
}




