use serde_json::Value;
use sqlx::{Pool, Sqlite};
use crate::manage_interesting_projects::get_id;

pub(crate)
struct IssueProperties {
  pub(crate) issue_id: u32,
  pub(crate) properties: Vec<(String /* key */, String /* value */)>
}

fn get_issues_properties(json_data: &Value) -> Result<Vec<IssueProperties>, String> {
  let Some(v) = json_data.get("issues") else {
    return Err(String::from("No field named 'issues' in the json"));
  };

  let Some(v) = v.as_array() else {
    return Err(String::from("Error: the fields named 'issues' isn't a json array"));
  };

  let properties = 
    v.iter()
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

        // todo ensure attachments are added here

        let key_values = fields
          .iter()
          .filter_map(|(key, value)| {
            match value.as_null() {
              Some(()) => { None },
              None => { Some((key.to_string(), value.to_string())) },
            }
          })
          .collect::<Vec<_>>();

        Some(IssueProperties {
          issue_id,
          properties: key_values,
        })
      })
      .collect::<Vec<_>>();
  
  Ok(properties)
}

pub(crate) 
async fn fill_issues_fields(json_data: &Value, db_conn: &mut Pool<Sqlite>) {
  let properties = get_issues_properties(&json_data);
  let Ok(properties) = properties else {
    eprintln!("Error: {e}", e = properties.err().unwrap());
    return;
  };

  // todo(perf): add detection of what is already in db and do some filter out. Here we happily
  // overwrite data with the exact same ones, thus taking the write lock on the
  // database for longer than necessary.
  // Plus it means the logs aren't that useful to troubleshoot how much data changed
  // in the database. Seeing messages saying
  // 'updated Issue fields in database: 58 rows were updated'
  // means there has been at most 58 changes. Chances are there are actually been
  // none since the last update.
  let query_str = "INSERT INTO IssueField (issue_id, field_id, field_value)
                      VALUES (?, ?, ?)
                      ON CONFLICT DO
                      UPDATE SET field_value = excluded.field_value;";

  let mut has_error = false;
  let mut row_affected = 0;
  let mut tx = db_conn
    .begin()
    .await
    .expect("Error when starting a sql transaction");


  for IssueProperties { issue_id, properties } in properties {
    for (key, value) in properties {
      let res = sqlx::query(query_str)
        .bind(issue_id)
        .bind(&key)
        .bind(&value)
        .execute(&mut *tx)
        .await;

      match res {
        Ok(e) => { row_affected += e.rows_affected() }
        Err(e) => {
          has_error = true;
          eprintln!("Error when adding an issue field with (issue_id {issue_id}, key: {key}, value: {value}): {e}");
        }
      }
    }
  }

  tx.commit().await.unwrap();

  if has_error {
    eprintln!("Error occurred while updating the database with issue fields")
  } else {
    eprintln!("updated Issue fields in database: {row_affected} rows were updated")
  }
}
