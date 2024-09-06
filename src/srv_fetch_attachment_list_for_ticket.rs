use base64::Engine;
use sqlx::{Error, FromRow, Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::server::Reply;

#[derive(FromRow)]
struct attachment_name_in_db {
  uuid: String,
  filename: String,
}

async fn get_ticket_attachment_list(issue_key: &str, db_conn: &mut Pool<Sqlite>) -> Result<String, String> {
  let query_str =
    "SELECT uuid, filename
     FROM Attachment
     WHERE issue_id = (select jira_id from Issue where Issue.key = ?)
     ORDER BY filename ASC;;"; // ordering used so it is easy to check for changes in the db

  let query_res = sqlx::query_as::<_, attachment_name_in_db>(query_str)
    .bind(issue_key)
    .fetch_all(&*db_conn)
    .await;

  match query_res {
    Ok(v) => {
      let base_64_encoded = v
        .into_iter()
        .map(|x| {
          let uuid = x.uuid;
          let filename_as_base64 = base64::engine::general_purpose::STANDARD.encode(x.filename.as_bytes());
          format!("{uuid}:{filename_as_base64}")
        })
        .reduce(|a, b| format!("{a},{b}"))
        .unwrap_or_default();
      Ok(base_64_encoded)
    }
    Err(e) => {
      Err(format!("Error occurred while querying the db for the list key values belonging to {issue_key}. Err: {e:?}"))
    }
  }
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

    let old_data = get_ticket_attachment_list(issue_key, db_conn).await;
    match &old_data {
      Ok(data) => if data.is_empty() {
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
      },
      Ok(data) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
      }
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }

    let mut db_conn = db_conn;
    let _ = add_details_to_issue_in_db(&config, issue_key, &mut db_conn).await;

    let new_data = get_ticket_attachment_list(issue_key, db_conn).await;
    match (&new_data, &old_data) {
      (Ok(new_data), Ok(old_data)) if new_data == old_data => {}
      (Ok(new_data), _) => if new_data.is_empty() {
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
      },
      (Ok(new_data), _) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {new_data}\n"))).await;
      }
      (Err(e), _) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }
  }
  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}