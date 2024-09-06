use sqlx::{Error, FromRow, Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::server::Reply;

#[derive(FromRow)]
struct keys_in_db {
  keys: String,
}

async fn get_ticket_list(db_conn: &mut Pool<Sqlite>) -> Result<String, String> {
  let query_str =
    "SELECT group_concat(key, ',') AS keys
     FROM (
       SELECT key
       FROM Issue
       ORDER BY jira_id ASC
     );"; // ordering used so it is easy to check for changes
                         // in the db
  let query_res = sqlx::query_as::<_, keys_in_db>(query_str)
    .fetch_one(&*db_conn)
    .await;
  
  match query_res {
    Ok(v) => { Ok(v.keys) }
    Err(e) => {
      Err(format!("Error occurred while querying the db for the list of jira keys. Err: {e:?}"))
    }
  }
}

pub(crate) async fn serve_fetch_ticket_list_request(config: Config,
                                                    request_id: &str,
                                                    out_for_replies: tokio::sync::mpsc::Sender<Reply>,
                                                    db_conn: &mut Pool<Sqlite>) {
  let _ = out_for_replies.send(Reply(format!("{request_id} ACK\n"))).await;

  let old_data = get_ticket_list(db_conn).await;
  match &old_data {
    Ok(data) if data.is_empty() => {
      // case where we didn't synchronise to the remote even once, or all tickets are
      // private, or none of the interesting projects exist
      let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
    }
    Ok(data) => {
      let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
    }
    Err(e) => {
      let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
    }
  }

  let mut db_conn = db_conn;
  let _ = update_interesting_projects_in_db(&config, &mut db_conn).await;

  let new_data = get_ticket_list(db_conn).await;
  match (&new_data, &old_data) {
    (Ok(new_data), Ok(old_data)) if new_data == old_data => {}
    (Ok(new_data), _) if new_data.is_empty() => {
      // case where everything got deleted
      let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
    },
    (Ok(new_data), _) => {
      let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {new_data}\n"))).await;
    }
    (Err(e), _) => {
      let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
    }
  }

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}