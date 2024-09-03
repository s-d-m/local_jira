use base64::Engine;
use sqlx::{Error, FromRow, Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::server::Reply;

#[derive(FromRow)]
struct attachment_data_in_db {
  content: Vec<u8>,
}

async fn get_attachment_content(uuid: &str, db_conn: &mut Pool<Sqlite>) -> Result<String, String> {
  let query_str =
    "SELECT content_data AS content
     FROM Attachment
     WHERE uuid = ?;";

  let query_res = sqlx::query_as::<_, attachment_data_in_db>(query_str)
    .bind(uuid)
    .fetch_optional(&*db_conn)
    .await;

  match query_res {
    Ok(None) => { Err(format!("No data found for file with uuid {uuid} in local database")) }
    Ok(Some(v)) => {
      let content_as_base64 = base64::engine::general_purpose::STANDARD.encode(v.content);
      Ok(content_as_base64)
    }
    Err(e) => {
      Err(format!("Error occurred while querying the db for content of file with uuid {uuid}. Err: {e:?}"))
    }
  }
}

pub(crate) async fn serve_fetch_attachment_content(request_id: &str,
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
    let uuid = splitted_params[0];

    let old_data = get_attachment_content(uuid, db_conn).await;
    match &old_data {
      Ok(data) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
      }
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }
  }
  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}