use std::collections::{HashMap, HashSet};
use base64::Engine;
use serde_json::{Map, Value};
use sqlx::{Error, FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_issue_details::{get_json_for_issue, IssueAttachment};
use crate::server::Reply;

#[derive(FromRow, Debug, Hash, PartialEq, Eq)]
struct key_value_in_db {
  field_key: String,
  field_value: String,
}


#[derive(FromRow)]
struct key_human_name {
  jira_field_key: String,
  human_name: String,
}

async fn get_key_human_hash_from_db(db_conn: &Pool<Sqlite>) -> Result<HashMap<String, String>, String> {
  // we need to get the uuid from the database.

  let query_str =
    "SELECT jira_id AS jira_field_key, human_name
     FROM Field;";

  let query_res = sqlx::query_as::<_, key_human_name>(query_str)
    .fetch_all(db_conn)
    .await;

  let query_res = match query_res {
    Ok(v) => { v }
    Err(e) => { return Err(format!("Error occurred while trying to fetch the list field key to human name from the Field table in the local db Err: {e:?}")) }
  };

  let res = query_res
    .into_iter()
    .map(|x| (x.jira_field_key, x.human_name))
    .collect::<HashMap<_, _>>();
  Ok(res)
}

fn format_key_value_list<'a>(kv_list: &'a [key_value_in_db], key_to_human: &'a HashMap<String, String>) -> String {

  let get_human_name = |key: &'a str| {
    let v = key_to_human.get(key);
    match v {
      Some(v) => v,
      None => {
        eprintln!("Error: can't find human name for field key {key} in local db.");
        key
      }
    }
  };

  let res = kv_list
    .iter()
    .map(|x| {
      let human_name = get_human_name(x.field_key.as_str());
      let key_as_bas64 = base64::engine::general_purpose::STANDARD.encode(human_name);
      let value_as_base64 = base64::engine::general_purpose::STANDARD.encode(x.field_value.as_bytes());
      format!("{key_as_bas64}:{value_as_base64}")
    })
    .reduce(|a, b| format!("{a},{b}"))
    .unwrap_or_default();

  res
}

async fn get_ticket_key_value_list_from_json(config: &Config, issue_key: &str) -> Result<Vec<key_value_in_db>, String> {
  let json = get_json_for_issue(config, issue_key).await;
  let json = match json {
    Ok(v) => {v}
    Err(e) => {
      return Err(format!("Failed to get json for issue {issue_key} while trying to get list of key value fields from remote. Err: {e:?}"));
    }
  };

  let fields = json
    .as_object()
    .and_then(|x| x.get("fields"))
    .and_then(|x| x.as_object());

  let fields = match fields {
    None => { return Err(format!("Error: failed to extract fields' list from json for issue {issue_key}"))}
    Some(v) => {v}
  };

  let res = fields
    .iter()
    .filter_map(|(key, value)| {
      if value.is_null() {
        None
      } else {
        let val = key_value_in_db {
          field_key: key.to_string(),
          field_value: value.to_string(),
        };
        Some(val)
      }
    })
    .collect::<Vec<_>>();

  Ok(res)
}

fn is_same_key_value_vector(param1: &[key_value_in_db], param2: &[key_value_in_db]) -> bool {
  // there should be enough key value fields in a ticket that the quadratic algorithms
  // starts taking more time. todo: verify this

  if param1.len() != param2.len() {
    return false;
  }

  let hashed_p1 = param1
    .iter()
    .collect::<HashSet<_>>();

  let hashed_p2 = param2
    .iter()
    .collect::<HashSet<_>>();

  let res = hashed_p1 == hashed_p2;
  res
}

async fn get_ticket_key_value_list_from_db(issue_key: &str, db_conn: &Pool<Sqlite>) -> Result<Vec<key_value_in_db>, String> {
  let query_str =
    "SELECT field_id AS field_key, field_value
     FROM IssueField
     WHERE issue_id = (SELECT jira_id FROM Issue WHERE Issue.key = ?);";

  let query_res = sqlx::query_as::<_, key_value_in_db>(query_str)
    .bind(issue_key)
    .fetch_all(db_conn)
    .await;

  match query_res {
    Ok(v) => {
      Ok(v)
    }
    Err(e) => {
      Err(format!("Error occurred while querying the db for the list key values belonging to {issue_key}. Err: {e:?}"))
    }
  }
}

pub(crate) async fn serve_fetch_ticket_key_value_fields(config: Config,
                                                    request_id: &str,
                                                    params: &str,
                                                    out_for_replies: tokio::sync::mpsc::Sender<Reply>,
                                                    db_conn: &mut Pool<Sqlite>) {
  let _ = out_for_replies.send(Reply(format!("{request_id} ACK\n"))).await;

  let splitted_params = params
    .split(',')
    .collect::<Vec<_>>();

  let nr_params = splitted_params.len();
  if nr_params != 1 {
    let err_msg = format!("{request_id} ERROR invalid parameters. FETCH_TICKET_KEY_VALUE_FIELDS need one parameter (the ticket id, like PROJ-123) but got {nr_params} instead. Params=[{params}]\n");
    let _ = out_for_replies.send(Reply(err_msg)).await;
  } else {
    let issue_key = splitted_params[0];

    let key_to_human = get_key_human_hash_from_db(db_conn).await;
    match key_to_human {
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR failed to get the mapping jira field key to human key from local db. Err: {e}\n"))).await;
      }
      Ok(key_to_human) => {
        let old_data = get_ticket_key_value_list_from_db(issue_key, db_conn).await;
        match &old_data {
          Ok(data) => {
            let base_64_encoded = format_key_value_list(data.as_slice(), &key_to_human);
            if base_64_encoded.is_empty() {
              // shouldn't happen since some key are necessary, e.g. "last updated", "summary", ...
              let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
            } else {
              let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {base_64_encoded}\n"))).await;
            }
          }
          Err(e) => {
            let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
          }
        }

        let new_data = get_ticket_key_value_list_from_json(&config, issue_key).await;

        match (&new_data, &old_data) {
          (Ok(new_data), Ok(old_data)) if is_same_key_value_vector(new_data, old_data) => {}
          (Ok(new_data), _) => {
            let base_64_encoded = format_key_value_list(new_data.as_slice(), &key_to_human);
            if base_64_encoded.is_empty() {
              // shouldn't happen since some key are necessary, e.g. "last updated", "summary", ...
              let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
            } else {
              let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {base_64_encoded}\n"))).await;
            }
          }
          (Err(e), _) => {
            let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
          }
        }
      }
    }
  }
  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}