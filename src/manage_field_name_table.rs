use std::collections::HashSet;
use std::hash::Hash;
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, Pool, Sqlite};
use sqlx::types::{Json, JsonValue};
use tokio::net::tcp::ReuniteError;
use crate::get_config::Config;
use crate::get_project_tasks_from_server::get_project_tasks_from_server;
use crate::manage_project_table::{get_projects_from_db, ProjectShortData};


fn get_fields_from_json<'a>(json_value: &Result<Value, String>) -> Result<Vec<(String, String)>, String> {
  let Ok(json_data) = json_value else {
    return Err(format!("Error: couldn't extract fields from json: {e}", e = json_value.clone().err().unwrap()));
  };

  let Some(v) = json_data.get("names") else {
    return Err(String::from("No field named 'names' in the json"));
  };

  let Some(v) = v.as_object() else {
    return Err(String::from("Error: the fields named 'names' isn't a json object"));
  };

  let res = v.iter()
    .filter_map(|(key, value)| {
      match value.as_str() {
        None => {
          println!("value of field in object named 'names' isn't a string");
          None
        }
        Some(s) => { Some((key.clone(), s.to_string())) }
      }
    })
    .collect::<Vec<_>>();

  Ok(res)
}

#[derive(FromRow, Hash, PartialEq, Eq, Debug)]
struct Issue {
  jira_id: u32,
  name: String
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
      Some(Issue { jira_id, name: key.to_string() })
    })
    .collect::<Vec<_>>();

  Ok(res)
}

#[derive(FromRow, Hash, PartialEq, Eq, Debug)]
struct IssueType {
  jira_id: u32,
  name: String,
  description: String,
}

fn merge_list<'a>(a: &(Vec<&'a Value>, HashSet<String>), b: &Vec<&'a Value>) -> (Vec<&'a Value>, HashSet<String>) {
  let mut res = a.clone();
  b.iter().for_each(|x| if res.1.insert(x.to_string()) {
    res.0.push(x);
  });
  return res;
}

fn get_all_unique<'a>(json_value: &'a Value, searched_key: &str) -> (Vec<&'a Value>, HashSet<String>) {
  let all_unique = match json_value {
    Value::Null
    | Value::Bool(_)
    | Value::Number(_)
    | Value::String(_) => { (Vec::new(), HashSet::new()) }
    Value::Array(a) => {
      let res = a.iter()
        .map(|x| get_all_unique(x, searched_key))
        .reduce(|a, b| merge_list(&a, &(b.0)));
      match res {
        None => { (Vec::new(), HashSet::new()) }
        Some(v) => { v }
      }
    },
    Value::Object(a) => {
      let filtered_values = a
        .iter()
        .map(|x|
          if x.0 == searched_key {
            let mut hashes = HashSet::new();
            let _ = hashes.insert(x.1.to_string());
            (vec![x.1], hashes)
          } else {
            get_all_unique(x.1, searched_key)
          }
        )
        .reduce(|a, b| merge_list(&a, &(b.0)));
      match filtered_values {
        None => { (Vec::new(), HashSet::new()) }
        Some(v) => { v }
      }
    }
  };
//  dbg!(&all_unique);
  all_unique
}

fn get_all_issue_type(json_value: &JsonValue) -> Vec<IssueType> {
  let res = get_all_unique(json_value, "issuetype");
  let res = res.0;
//  dbg!(&res);
  let res = res
    .into_iter()
    .filter_map(|x| {
//      dbg!(x);
      let Some(map) = x.as_object() else {
        return None;
      };
      let Some(id) = map.get("id") else {
        return None;
      };
      let Some(id) = id.as_str() else {
        return None;
      };
      let Ok(id) = id.parse::<u32>() else {
        return None;
      };
      let Some(name) = map.get("name") else {
        return None;
      };
      let Some(name) = name.as_str() else {
        return None;
      };
      let Some(description) = map.get("description") else {
        return None;
      };
      let Some(description) = description.as_str() else {
        return None;
      };
      Some(IssueType {
        jira_id: id,
        name: name.to_string(),
        description: description.to_string(),
      })
    })
    .collect::<Vec<_>>();
//  dbg!(&res);
  res
}

fn get_issue_type_from_json(json_value: &Result<Value, String>) -> Result<Vec<IssueType>, String> {
  match json_value {
    Ok(json_value) => {
      let res = get_all_issue_type(&json_value);
      Ok(res)
    }
    Err(e) => { Err(format!("Err: input is not a proper json value: Got [{e}]")) }
  }
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

async fn get_issue_type_from_db(db_conn: &Pool<Sqlite>) -> Result<Vec<IssueType>, String> {
  let query_str =
    "SELECT  jira_id, name, description
     FROM IssueType;";

  let rows = sqlx::query_as::<_, IssueType>(query_str)
    .fetch_all(db_conn)
    .await;

  rows.map_err(|e| {
    format!("Error occurred while trying to get Issue types from local database: {e}", e = e.to_string())
  })
}


fn get_fields_not_in_db<'a, 'b>(fields: &'a [(String, String)], fields_in_db: &'b [(String, String)])
                                -> Vec<&'a (String, String)>
  where 'b: 'a
{
  // use hash tables to avoid quadratic algorithm
  // todo(perf) use faster hasher. We don't need the security from SIP
  let to_hash_set = |x: &'a [(String, String)]| {
    x
      .iter()
      .collect::<HashSet<&'a (String, String)>>()
  };

  let fields_in_db = to_hash_set(fields_in_db);
  let fields = to_hash_set(fields);

  let res = fields.difference(&fields_in_db)
    .map(|x| *x)
    .collect::<Vec<_>>();
  res
}

#[derive(FromRow, Debug)]
pub(crate) struct fields_in_db {
  jira_field_name: String,
  human_name: String,
}

pub(crate) async fn get_fields_from_db(db_conn: &Pool<Sqlite>) -> Vec<fields_in_db> {
  let query_str =
    "SELECT  jira_field_name, human_name
         FROM fields_name;";

  let rows = sqlx::query_as::<_, fields_in_db>(query_str)
    .fetch_all(db_conn)
    .await;

  rows.unwrap_or_else(|e| {
    eprintln!("Error occurred while trying to get projects from local database: {e}");
    Vec::new()
  })
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

  for Issue { jira_id, name: key } in issues_to_insert {
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

async fn update_issue_types_in_db(issue_types_to_insert: &Vec<IssueType>, db_conn: &mut Pool<Sqlite>) {
  let issue_types_in_db = get_issue_type_from_db(db_conn).await;
  let issue_types_in_db = match issue_types_in_db {
    Ok(v) => { v }
    Err(e) => {
      println!("Error occurred: {e}");
      Vec::new()
    }
  };

  let hashed_issues_in_db = issue_types_in_db.iter().collect::<HashSet<&IssueType>>();
  let issue_types_to_insert = issue_types_to_insert
    .iter()
    .filter(|x| !hashed_issues_in_db.contains(x))
    .collect::<Vec<_>>();

  if issue_types_to_insert.is_empty() {
    println!("No new Issue type found");
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

  for IssueType { jira_id, name, description } in issue_types_to_insert {
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
    println!("Error occurred while updating the database with Issue types")
  } else {
    println!("updated Issue types in database: {row_affected} rows were updated")
  }
}


pub(crate) async fn update_field_names_in_db(fields: &[(String, String)], db_conn: &Pool<Sqlite>) {
  if fields.is_empty() {
    // no need to query the db to find out that there won't be any project to insert there
    return;
  }

  // avoid taking write locks on the db if there is nothing to update
  let fields_in_db = get_fields_from_db(db_conn).await;
  let fields_in_db = fields_in_db
    .into_iter()
    .map(|x| (x.jira_field_name.clone(), x.human_name.clone()))
    .collect::<Vec<_>>();

  let fields_to_insert = get_fields_not_in_db(fields, &fields_in_db);

  // dbg!(&fields_to_insert);
  // dbg!(&fields_in_db);


  if fields_to_insert.is_empty() {
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
    "INSERT INTO fields_name (jira_field_name, human_name) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET human_name = excluded.human_name";

  for (jira_id, human_name) in fields_to_insert {
    let res = sqlx::query(query_str)
      .bind(jira_id)
      .bind(human_name)
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
    println!("Error occurred while updating the database with projects")
  } else {
    println!("updated projects in database: {row_affected} rows were updated")
  }
}

pub(crate) async fn update_interesting_projects_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  for project_key in config.interesting_projects() {
    let json_tickets = get_project_tasks_from_server(project_key, &config).await;
    let fields = get_fields_from_json(&json_tickets);
    if let Ok(fields) = fields {
      update_field_names_in_db(&fields, db_conn).await;
    }

    let issue_types = get_issue_type_from_json(&json_tickets);
    if let Ok(issue_types) = issue_types {
      update_issue_types_in_db(&issue_types, db_conn).await;
    }

    let issues = get_issues_from_json(json_tickets);
    if let Ok(issues) = issues {
      update_issues_in_db(&issues, db_conn).await;
    }
  }
}




