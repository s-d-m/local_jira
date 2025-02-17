use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Error, FromRow, Pool, query, Sqlite};
use sqlx::types::{Json, JsonValue};
use tokio::net::tcp::ReuniteError;
use tokio::task::JoinSet;
use crate::get_config::Config;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::get_project_tasks_from_server::get_project_tasks_from_server;
use crate::manage_issue_field::fill_issues_fields_from_json;
use crate::manage_project_table::Project;


#[derive(FromRow, Hash, PartialEq, Eq, Debug)]
pub(crate)
struct Issue {
  pub(crate) jira_id: u32,
  pub(crate) key: String,
  pub(crate) project_key: String,
}

fn get_issues_from_json(json_data: &Value, project_key: &str) -> Result<Vec<Issue>, String> {
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
      let Some(key) = key.as_str() else {
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
      Some(Issue { jira_id, key: key.to_string(), project_key: project_key.to_string() })
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
    "SELECT  jira_id, key, project_key
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


pub(crate) async fn update_issues_in_db(issues_to_insert: &Vec<Issue>, db_conn: &mut Pool<Sqlite>, project_key: &str) {
  let issues_in_db = get_issues_from_db(&db_conn).await;
  let issues_in_db = match issues_in_db {
    Ok(v) => {v}
    Err(e) => {
      eprintln!("Error occurred: {e}");
      return
    }
  };

  let hashed_issues_in_db = issues_in_db.iter().collect::<HashSet<&Issue>>();
  let issues_to_insert = issues_to_insert
    .iter()
    .filter(|x| !hashed_issues_in_db.contains(x))
    .collect::<Vec<_>>();

  match issues_to_insert.is_empty() {
    true => { eprintln!("No new issue found for project [{project_key}]") }
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
        "INSERT INTO Issue (jira_id, key, project_key) VALUES
                (?, ?, ?)
            ON CONFLICT DO
            UPDATE SET key = excluded.key,
                       project_key = excluded.project_key";

      for Issue { jira_id, key, project_key } in issues_to_insert {
        let res = sqlx::query(query_str)
          .bind(jira_id)
          .bind(key)
          .bind(project_key)
          .execute(&mut *tx)
          .await;
        match res {
          Ok(e) => { row_affected += e.rows_affected() }
          Err(e) => {
            has_error = true;
            eprintln!("Error when adding (jira_id {jira_id}, key: {key}, project_key: {project_key}): {e}")
          }
        }
      }

      tx.commit().await.unwrap();

      if has_error {
        eprintln!("Error occurred while updating the database with Issue")
      } else {
        eprintln!("updated Issues in database: {row_affected} rows were updated")
      }
    }
  }
}

#[derive(FromRow, Clone, Debug, Hash, Eq, PartialEq)]
pub(crate) struct IssueLink {
  pub(crate) jira_id: u32,
  pub(crate) link_type_id: u32,
  pub(crate) outward_issue_id: u32,
  pub(crate) inward_issue_id: u32,
}

pub(crate) fn get_id(json_data: &Value) -> Option<u32> {
  let Some(json_data) = json_data.as_object() else {
    return None;
  };

  let Some(id) = json_data.get("id") else {
    return None;
  };

  let Some(id) = id.as_str() else {
    return None;
  };

  let Ok(id) = id.parse::<u32>() else {
    return None;
  };

  Some(id)
}

fn get_link_type(json_data: &Value) -> Option<(u32 /* link id */, bool /* is outward */, u32 /* other issue id */, u32 /* link type id */)> {
  let Some(link_id) = get_id(json_data) else {
    return None;
  };

  let Some(json_data) = json_data.as_object() else {
    return None;
  };

  let Some(link_type) = json_data.get("type") else {
    return None;
  };
  let Some(link_type_id) = get_id(link_type) else {
    return None;
  };
//  dbg!(json_data);
  let inward = json_data.get("inwardIssue");
  let outward = json_data.get("outwardIssue");

  let res = match (outward, inward) {
    (Some(_), Some(_)) => {
      eprintln!("Error a link can't be both outward and inward");
      None
    }
    (Some(outward), None) => {
      Some((true, outward))
    }
    (None, Some(inward)) => {
      Some((false, inward))
    }
    (None, None) => {
      eprintln!("Error a link has to be either inward or outward. Can't be none.");
      None
    }
  };

  let Some((is_outward, other_issue)) = res else {
    return None;
  };

  let Some(other_issue_id) = get_id(other_issue) else {
    return None;
  };

  Some((link_id, is_outward, other_issue_id, link_type_id))
}

pub(crate) fn get_issue_links_from_json(json_data: &Value) -> Result<Vec<IssueLink>, String> {
  let Some(v) = json_data.get("issues") else {
    return Err(String::from("No field named 'issues' in the json"));
  };

  let Some(v) = v.as_array() else {
    return Err(String::from("Error: the fields named 'issues' isn't a json array"));
  };

  let issue_links = v
    .into_iter()
    .filter_map(|x| {
      let Some(issue_id) = get_id(x) else {
        return None;
      };
      let Some(x) = x.as_object() else {
        return None;
      };
      let Some(fields) = x.get("fields") else {
        return None;
      };
      let Some(fields) = fields.as_object() else {
        return None;
      };
      let Some(issue_links) = fields.get("issuelinks") else {
        return None;
      };
      let Some(issue_links) = issue_links.as_array() else {
        return None;
      };

      let mut res = Vec::new();
      for link in issue_links {
        if let Some((link_id, is_outward, other_issue_id, link_type_id)) = get_link_type(link) {
          let (inward_issue_id, outward_issue_id) = if is_outward {
            (issue_id, other_issue_id)
          } else {
            (other_issue_id, issue_id)
          };
          if inward_issue_id > outward_issue_id { // to only add one of the two opposite link
            res.push(IssueLink {
              jira_id: link_id,
              link_type_id,
              outward_issue_id,
              inward_issue_id,
            })
          }
        };
      }
      Some(res)
    })
    .flatten()
    .collect::<Vec<_>>();

  Ok(issue_links)
}

async fn get_links_from_db(jira_ids: &[u32], db_conn: &mut Pool<Sqlite>) -> HashSet<IssueLink>
{
  let mut res = HashSet::new();
  let query_str =
  "SELECT  jira_id, link_type_id, outward_issue_id, inward_issue_id
  FROM IssueLink
  WHERE outward_issue_id = ?
     OR inward_issue_id = ?";

  for id in jira_ids {
    let query_res = sqlx::query_as::<_, IssueLink>(query_str)
      .bind(id)
      .bind(id)
      .fetch_all(&*db_conn)
      .await;

    match query_res {
      Ok(e) => {
        res.extend(e.into_iter());
      }
      Err(e) => {
        eprintln!("Error occurred while retrieving links for issue with id {id} from local db. Err: {e}")
      }
    }
  }
  res
}


pub(crate) async fn update_issue_links_in_db(issues_ids: &[u32], issue_links: &Vec<IssueLink>, db_conn: &mut Pool<Sqlite>) {
  //dbg!(&issue_links);
  if issue_links.is_empty() {
    return;
  }

  let links_from_db = get_links_from_db(&issues_ids, db_conn).await;
  let links_from_remote = issue_links
    .iter()
    .map(|a| a.clone())
    .collect::<HashSet<_>>();
  let links_in_db_not_in_remote = links_from_db.difference(&links_from_remote)
    .collect::<Vec<_>>();
  let links_in_remote_not_in_db = links_from_remote.difference(&links_from_db)
    .collect::<Vec<_>>();
  let links_to_remove = links_in_db_not_in_remote;
  let links_to_insert = links_in_remote_not_in_db;

  match links_to_remove.is_empty() {
    true => {eprintln!("No links found in local db that were removed in server")}
    false => {
      let mut has_error = false;
      let mut row_affected = 0;
      let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

      let query_str =
        "DELETE FROM IssueLink
        WHERE jira_id = ?";

      for &IssueLink{ jira_id, link_type_id, outward_issue_id, inward_issue_id } in links_to_remove {
        let res = sqlx::query(query_str)
          .bind(jira_id)
          .execute(&mut *tx)
          .await;
        match res {
          Ok(e) => { row_affected += e.rows_affected() }
          Err(e) => {
            has_error = true;
            eprintln!("Error while deleting from attachment table: {e}")
          }
        }
      }

      tx.commit().await.unwrap();

      if has_error {
        eprintln!("Error occurred while removing out-of-date issue links in the local database")
      } else {
        eprintln!("updated IssueLinks in database: {row_affected} rows were removed")
      }
    }
  }

  match links_to_insert.is_empty() {
    true => {eprintln!("No new link between issues found on the remote server")}
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
        "INSERT INTO IssueLink (jira_id, link_type_id, outward_issue_id, inward_issue_id) VALUES
                (?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET link_type_id = excluded.link_type_id,
                       outward_issue_id = excluded.outward_issue_id,
                       inward_issue_id = excluded.inward_issue_id";

      for &IssueLink { jira_id, link_type_id, outward_issue_id, inward_issue_id } in links_to_insert {
        let res = sqlx::query(query_str)
          .bind(jira_id)
          .bind(link_type_id)
          .bind(outward_issue_id)
          .bind(inward_issue_id)
          .execute(&mut *tx)
          .await;
        match res {
          Ok(e) => { row_affected += e.rows_affected() }
          Err(e) => {
            has_error = true;
            eprintln!("Error when adding (jira_id {jira_id}, link_type_id: {link_type_id}, outward_issue_id: {outward_issue_id}, inward_issue_id: {inward_issue_id}): {e}")
          }
        }
      }

      tx.commit().await.unwrap();

      if has_error {
        eprintln!("Error occurred while updating the database with IssueLinks")
      } else {
        eprintln!("updated IssueLinks in database: {row_affected} rows were inserted")
      }
    }
  }
}

async fn initialise_given_project_in_db(config: Config, project_key: String, mut db_conn: Pool<Sqlite>) {
  let json_tickets = get_project_tasks_from_server(project_key.as_str(), &config).await;
  let mut db_handle = db_conn.clone();

  if let Ok(paginated_json_tickets) = json_tickets {
    let issues_and_links = paginated_json_tickets
      .iter()
      .map(|json_tickets| {
        let issues = get_issues_from_json(&json_tickets, project_key.as_str());
        let links = get_issue_links_from_json(&json_tickets);
        (json_tickets, issues, links)
      })
      .collect::<Vec<_>>();

    for (json_tickets, issues, _links) in &issues_and_links {
      match issues {
        Ok(issues) => {
          update_issues_in_db(&issues, &mut db_handle, project_key.as_str()).await;
        }
        Err(e) => { eprintln!("Error: {e}"); }
      }

      fill_issues_fields_from_json(&json_tickets, &mut db_handle).await;
    }

    // First insert all issues in the db, and then insert the links between issues.
    // This avoids the issues where inserting links fails due to foreign constraints violation
    // at the database layer because some issues are linked to others which crosses a pagination
    // limit.
    for (json_tickets, issues, links) in &issues_and_links {
      match (issues, links) {
        (Ok(issues), Ok(issue_links)) => {
          let issues_id = issues
            .iter()
            .map(|x| x.jira_id)
            .collect::<Vec<_>>();
          update_issue_links_in_db(issues_id.as_slice(), &issue_links, &mut db_handle).await;
        }
        (_, Err(e)) => { eprintln!("Error: {e}") }
        (Err(e), Ok(_)) => { eprintln!("Not updating links due to former error {e}")}
      }
    }

    let issues_keys = issues_and_links
      .iter()
      .filter_map(|(json_tickets, issues, links)| {
        match issues {
          Ok(a) => {Some(a.iter())}
          Err(_) => {None}
        }
      })
      .flatten()
      .map(|x| &x.key)
      .collect::<Vec<_>>();

    for key in issues_keys {
      add_details_to_issue_in_db(&config,
                                 &key,
                                 &mut db_conn).await
    }
  }
}

pub(crate) async fn initialise_interesting_projects_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
  let interesting_projects = config.interesting_projects();

  let mut tasks = interesting_projects
    .iter()
    .map(|x| tokio::spawn(initialise_given_project_in_db(config.clone(), x.clone(), db_conn.clone())))
    .collect::<JoinSet<_>>();

  while let Some(res) = tasks.join_next().await {
  }
}

