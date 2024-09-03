use sqlx::{Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::server::Reply;

pub(crate) async fn serve_synchronise_updated_tickets(config: Config,
                                             request_id: &str,
                                             out_for_replies: tokio::sync::mpsc::Sender<Reply>,
                                             db_conn: &mut Pool<Sqlite>) {
  let _ = out_for_replies.send(Reply(format!("{request_id} ACK\n"))).await;

  let mut db_conn = db_conn;
  update_interesting_projects_in_db(&config, &mut db_conn).await;

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}