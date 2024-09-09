use base64::Engine;
use sqlx::{Error, FromRow, Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::{add_details_to_issue_in_db, get_ticket_attachment_list_from_json, IssueAttachment};
use crate::server::Reply;


#[derive(FromRow)]
struct attachment_name_in_db {
  uuid: String,
  filename: String,
}


async fn get_ticket_attachments_uuid_and_name_from_db(issue_key: &str, db_conn: &mut Pool<Sqlite>) -> Result<Vec<attachment_name_in_db>, String> {
  let query_str =
    "SELECT uuid, filename
     FROM Attachment
     WHERE issue_id = (SELECT jira_id FROM Issue WHERE Issue.key = ?)
     ORDER BY filename ASC;"; // ordering used so it is easy to check for changes in the db

  let query_res = sqlx::query_as::<_, attachment_name_in_db>(query_str)
    .bind(issue_key)
    .fetch_all(&*db_conn)
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

#[derive(FromRow)]
struct uuid_id {
  uuid: String,
  id: i64,
}

async fn add_uuid_to_names(attachment_list: &[IssueAttachment], issue_key: &str, db_conn: &Pool<Sqlite>) -> Result<Vec<attachment_name_in_db>, String> {
  // we need to get the uuid from the database.

  let query_str =
    "SELECT uuid, id
     FROM Attachment
     WHERE issue_id = (SELECT jira_id FROM Issue WHERE Issue.key = ?);";

  let query_res = sqlx::query_as::<_, uuid_id>(query_str)
    .bind(issue_key)
    .fetch_all(&*db_conn)
    .await;

  let uuid_id_in_db = match query_res {
    Ok(v) => { v }
    Err(e) => { return Err(format!("Error occurred while trying to get the uuid for issue key {issue_key} from db. Err: {e:?}")) }
  };

  let get_uuid_for_id = |id| {
    let uuid = uuid_id_in_db
      .iter()
      .filter_map(|x| if x.id == id { Some(&x.uuid) } else { None })
      .nth(0);
    uuid
  };

  let with_uuid = attachment_list
    .into_iter()
    .map(|x| {
      let id = x.attachment_id;
      let filename = &x.filename;
      let uuid = get_uuid_for_id(id);
      match uuid {
        None => { Err(format!("local db has no uuid for attachment of {issue_key} with id={id} and name {filename}")) }
        Some(uuid) => {
          Ok(attachment_name_in_db {
            uuid: uuid.to_string(),
            filename: x.filename.to_string()
          })
        }
      }
    }).collect::<Vec<_>>();

  let errors = with_uuid
    .iter()
    .filter_map(|x| match x {
      Ok(_) => {None}
      Err(e) => {Some(e.to_string())}
    })
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or_default();

  if !errors.is_empty() {
    return Err(errors)
  };

  let result = with_uuid
    .into_iter()
    .filter_map(|x| match x {
      Ok(e) => {Some(e)}
      Err(_) => {None}
    })
    .collect::<Vec<_>>();

  Ok(result)
}

async fn get_ticket_attachments_uuid_and_name_from_remote(issue_key: &str, config: &Config, db_conn: &Pool<Sqlite>) -> Result<Vec<attachment_name_in_db>, String> {
  // todo: start a db update here, and either
  //   1. cancel it if the first with uuid works.
  //   2. await it at the update_interesting_project_in_db await point below
  // This would make this query about 1s faster to finish in the case where the
  // update db would have been triggered (1s is about the time to retrieve the ticket's json)
  // and not negatively impact much the other cases.

  let attachment_list = get_ticket_attachment_list_from_json(issue_key, config).await;
  let attachment_list = match attachment_list {
    Ok(v) => {v}
    Err(e) => { return Err(e)}
  };

  let with_uuid = add_uuid_to_names(attachment_list.as_slice(),
                                    issue_key, db_conn).await;

  match with_uuid {
    Ok(v) => {
      return Ok(v)
    }
    Err(e) => {
      eprintln!("{e}\nTriggering a database update now to see if those issues fix themselves and retry");
    }
  }

  let _ = update_interesting_projects_in_db(&config, &db_conn).await;

  let with_uuid = add_uuid_to_names(attachment_list.as_slice(),
                                    issue_key, db_conn).await;

  with_uuid
}

fn are_attachment_names_equal(param1: &[attachment_name_in_db], param2: &[attachment_name_in_db]) -> bool {
  if param1.len() != param2.len() {
    return false;
  }

  // numbers of attachments per tickets should be low enough that the
  // quadratic algorithm beats the one using hash tables.
  // todo: ensure that or add another code paths decided by the number or attachments
  let is_elt_in_list = |elt: &attachment_name_in_db, list: &[attachment_name_in_db]| {
    let res = list
      .iter()
      .any(|x| (x.uuid == elt.uuid) && (x.filename == elt.filename));
    res
  };

  let is_same = param1
    .iter()
    .all(|x| is_elt_in_list(x, param2));

  is_same
}
fn format_attachment_list(attachment_list: &[attachment_name_in_db]) -> String {
    let base_64_encoded = attachment_list
        .iter()
        .map(|x| {
          let uuid = &x.uuid;
          let filename_as_base64 = base64::engine::general_purpose::STANDARD.encode(x.filename.as_bytes());
          format!("{uuid}:{filename_as_base64}")
        })
        .reduce(|a, b| format!("{a},{b}"))
        .unwrap_or_default();

    base_64_encoded
}

pub(crate) async fn serve_fetch_ticket_attachment_list(config: Config,
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
    let err_msg = format!("{request_id} ERROR invalid parameters. FETCH_ATTACHMENT_LIST_FOR_TICKET need one parameter (the ticket id, like PROJ-123) but got {nr_params} instead. Params=[{params}]\n");
    let _ = out_for_replies.send(Reply(err_msg)).await;
  } else {
    let issue_key = splitted_params[0];

    let old_data = get_ticket_attachments_uuid_and_name_from_db(issue_key, db_conn).await;
    match &old_data {
      Ok(data) => {
        let formatted = format_attachment_list(data.as_slice());
        if formatted.is_empty() {
          let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
        } else {
          let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {formatted}\n"))).await;
        }
      }
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }

    let new_data = get_ticket_attachments_uuid_and_name_from_remote(issue_key, &config, db_conn).await;
    match (&new_data, &old_data) {
      (Ok(new_data), Ok(old_data)) if are_attachment_names_equal(new_data, old_data) => {}
      (Ok(new_data), _) => {
        let formatted = format_attachment_list(new_data.as_slice());
        if formatted.is_empty() {
          let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
        } else {
          let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {formatted}\n"))).await;
        }
        // todo: run a background synchronisation since we know there has been changes
      },
      (Err(e), _) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }
  }
  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}