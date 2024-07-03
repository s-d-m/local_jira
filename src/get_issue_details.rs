use html2text::parse;
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_interesting_projects::get_id;
use crate::manage_issue_field::IssueProperties;
use serde_json::Value;
use sqlx::types::JsonValue;
use sqlx::{Error, FromRow, Pool, Sqlite};

async fn get_one_json(config: &Config, issue_keu: &str) -> Result<JsonValue, String> {
    let query = format!("/rest/api/3/issue/{issue_keu}");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get detail for issue {issue_keu} from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };
    Ok(json_data)
}

async fn get_properties_from_json(
    issue_key: &str,
    json_data: &Value,
) -> Result<IssueProperties, String> {
    let issue_id = get_id(json_data);
    let Some(issue_id) = issue_id else {
        return Err(format!(
            "error: the json data for {issue_key} does not contain an \"id\" fields"
        ));
    };

    let Some(json) = json_data.as_object() else {
        return Err(format!(
            "error: received data is not a json object. Got {}",
            json_data.to_string()
        ));
    };

    let Some(fields) = json.get("fields") else {
        return Err(format!(
            "error: received json for issue {issue_key} does not contain a field named \"fields\"."
        ));
    };

    let Some(fields) = fields.as_object() else {
        return Err(format!(
            "error: the field named \"fields\" for {issue_key} is not a json object."
        ));
    };

    let key_values = fields
        .iter()
        .filter_map(|(key, value)| match value.as_null() {
            Some(()) => None,
            None => Some((key.to_string(), value.to_string())),
        })
        .collect::<Vec<_>>();

    let res = IssueProperties {
        issue_id,
        properties: key_values,
    };

    Ok(res)
}

async fn insert_properties_into_db(issue_properties: &IssueProperties, db_conn: &mut Pool<Sqlite>) {
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

    let issue_id = issue_properties.issue_id;
    for (key, value) in &issue_properties.properties {
        let res = sqlx::query(query_str)
            .bind(issue_id)
            .bind(&key)
            .bind(&value)
            .execute(&mut *tx)
            .await;

        match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
                has_error = true;
                eprintln!("Error when adding an issue field with (issue_id {issue_id}, key: {key}, value: {value}): {e}");
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

#[derive(FromRow, Debug)]
struct AttachmentValue {
    field_value: String
}

#[derive(Debug)]
struct IssueAttachment {
    attachment_id: i64,
    filename: String,
    mime_type: String,
    size: Option<i64>,
}

async fn get_attachments_in_db_for_issue(issue_id: u32, config: &Config, db_conn: &mut Pool<Sqlite>) -> Vec<IssueAttachment> {
  let query_str = "SELECT field_value
                          FROM IssueField
                          WHERE     (IssueField.issue_id == ?)
                                AND (IssueField.field_id == \"attachment\");";

  let query_res = sqlx::query_as::<_, AttachmentValue>(query_str)
    .bind(issue_id)
    .fetch_optional(&*db_conn)
    .await;
  let attachment_value = match query_res {
      Ok(v) => {v}
      Err(e) => {
          eprintln!("{}", e);
          return vec![];
      }
  };

    let Some(attachment_value) = attachment_value else {
        // issue has no attachment.
        return vec![];
    };

    let json: Result<Value, serde_json::error::Error> = serde_json::from_str(&attachment_value.field_value);
    let Ok(json) = json else {
        eprintln!("Error: json data for the attachment of {issue_id} is not valid json. Got {}", json.err().unwrap().to_string());
        return vec![]
    };

    let Some(json) = json.as_array() else {
        eprintln!("Error: json data for the attachment of {issue_id} is not an array. Got {}", json.to_string());
        return vec![];
    };

    let res = json
      .iter()
      .filter_map(|x| {
          let attachment_id = x
            .get("id")
            .and_then(Value::as_str)
            .and_then(|a| {
                let val = str::parse::<i64>(a);
                val.ok()
            });
          let Some(attachment_id) = attachment_id else {
              eprintln!("couldn't find an id for attachment");
              return None;
          };

          let filename = x
            .get("filename")
            .and_then(Value::as_str);
          let Some(filename) = filename else {
              eprintln!("couldn't find the filename of the attachment");
              return None;
          };

          let mime_type = x
            .get("mimeType")
            .and_then(Value::as_str);
          let Some(mime_type) = mime_type else {
              eprintln!("couldn't find the mime type of the attachment");
              return None;
          };

          let size = x
            .get("size")
            .and_then(Value::as_i64);

          Some(IssueAttachment {
              attachment_id,
              filename: filename.to_string(),
              mime_type: mime_type.to_string(),
              size,
          })
      })
      .collect::<Vec<_>>();

  res
}

#[derive(FromRow)]
struct AttachmentId {
  id: i64,
}

struct AttachmentWithFileDetails {
  attachment_id: i64,
  filename: String,
  mime_type: String,
  size: Option<i64>,
  uuid: String,
  issue_id: u32,
}

fn add_details_to_attachment(issue_id: u32, attachment: IssueAttachment) -> AttachmentWithFileDetails {
  // the uuid extraction is based on what jira does internally.
  // When a ticket has an attachment, the json of that ticket will contain:
  // attachment: "basename<space><open parentheses>uuid<closing paren><dot>extension
  // the question is therefore: what happens when:
  //   - a filename doesn't have an extension
  //   - a filename contains parentheses in the extension
  // ?
  //
  // Turns out, not all files contains a uuid in there. It looks like only those
  // which are fully 'inlined' (or previewed) in messages get a uuid.

  let begin_uuid = attachment.filename.rfind('(');
  let end_uuid = attachment.filename.rfind(')');

  let uuid = match (begin_uuid, end_uuid) {
    (Some(b), Some(e)) if e == b + 37 => {
      // 37 == sizeof uuid
      &attachment.filename[(b+1)..e]
    },
    _ => ""
  };

  let uuid = uuid.to_string();
  let attachment_id = attachment.attachment_id;

  AttachmentWithFileDetails {
    attachment_id,
    filename: attachment.filename,
    mime_type: attachment.mime_type,
    size: attachment.size,
    uuid,
    issue_id,
  }

}

async fn update_attachments_in_db(config: &Config, issue_id: u32, attachments: Vec<IssueAttachment>, db_conn: &mut Pool<Sqlite>) {
  // retrieve the attachments saved in the db belonging to the issue
  // then delete those which got deleted since the last db update
  // and download the files which weren't already downloaded
  let query_str = "SELECT id FROM Attachment WHERE issue_id == ?;";
  let query_res = sqlx::query_as::<_, AttachmentId>(query_str)
    .bind(issue_id)
    .fetch_all(&*db_conn)
    .await;
  let Ok(query_res) = query_res else {
    eprintln!("Error while retrieving the already known attachments for issue with id {issue_id}. Error: {e}",
    e = query_res.err().unwrap().to_string());
    return;
  };

  // find the files which are no longer attached to issue_id. The attachments parameters is the
  // latest value. A linear time solution would be:
  // 1. create two hash tables
  // 2. insert the ids of the old data (from the db) in one hash table
  // 3. insert the ids of the new data (from the jira server) in the other hash table
  // 4. use the difference function to find the ids in one db but not in the other.
  // this is what the `get_inputs_not_in_db` does. Here, the amount of attached
  // files per ticket is expected to be low enough that the simple quadratic algorithm
  // should be plenty fast, and in fact even faster than the linear algorithm due to
  // avoiding memory allocation and having a better use of CPU caches.

  let ids_in_db_not_in_server = query_res
    .iter()
    .filter(|a| {
      let is_in_server = attachments
        .iter()
        .any(|x| x.attachment_id == a.id);
      !is_in_server
    })
    .collect::<Vec<_>>();

  let (mut has_error, mut row_affected) = delete_attachments_in_db_but_not_in_server(db_conn, ids_in_db_not_in_server).await;



  // Add attachments which are in the remote server but not yet in the database
  let query_str =
    "INSERT INTO Attachment (uuid, id, issue_id, filename, mime_type, file_size)
     VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT DO
     UPDATE SET
       uuid = excluded.uuid,
       id = excluded.id,
       issue_id = excluded.issue_id,
       filename = excluded.filename,
       mime_type = excluded.mime_type,
       file_size = excluded.file_size;";

  let ids_in_server_not_in_db = attachments
    .into_iter()
    .filter(|a| {
      let is_in_db = query_res
        .iter()
        .any(|x| x.id == a.attachment_id);
      !is_in_db
    })
    .collect::<Vec<_>>();

  let ids_in_server_not_in_db = ids_in_server_not_in_db
    .into_iter()
    .map(|x| add_details_to_attachment(issue_id, x))
    .collect::<Vec<_>>();

  let mut tx = db_conn
    .begin()
    .await
    .expect("Error when starting a sql transaction");

  for attachment in ids_in_server_not_in_db {
    let res = sqlx::query(query_str)
      .bind(attachment.uuid)
      .bind(attachment.attachment_id)
      .bind(attachment.issue_id)
      .bind(attachment.filename)
      .bind(attachment.mime_type)
      .bind(attachment.size)

      .execute(&mut *tx)
      .await;
    match res {
      Ok(e) => { row_affected += e.rows_affected() }
      Err(e) => {
        has_error = true;
        eprintln!("Error while inserting into attachment table: {e}")
      }
    }
  }
  tx.commit().await.unwrap();

}

async fn delete_attachments_in_db_but_not_in_server(db_conn: &mut Pool<Sqlite>, ids_in_db_not_in_server: Vec<&AttachmentId>) -> (bool, u64) {
  // delete attachments which are in the db, but not on the remote server
  // anymore.
  let mut has_error = false;
  let mut row_affected = 0;

  let query_str =
    "DELETE FROM Attachment
     WHERE id == (?);";

  let mut tx = db_conn
    .begin()
    .await
    .expect("Error when starting a sql transaction");

  for id in ids_in_db_not_in_server {
    let res = sqlx::query(query_str)
      .bind(id.id)
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
  (has_error, row_affected)
}

pub(crate) async fn add_details_to_issue_in_db(
    config: &Config,
    issue_keu: &str,
    db_conn: &mut Pool<Sqlite>,
) {
    let json = get_one_json(config, issue_keu).await;
    let Ok(json) = json else {
        eprintln!("{}\n", json.err().unwrap());
        return;
    };
    let properties = get_properties_from_json(issue_keu, &json).await;
    let properties = match properties {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}\n", e);
            return;
        }
    };
    let issue_id = properties.issue_id;

    insert_properties_into_db(&properties, db_conn).await;
    let attachments = get_attachments_in_db_for_issue(issue_id, config, db_conn).await;
    update_attachments_in_db(config, issue_id, attachments, db_conn).await;
}