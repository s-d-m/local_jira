use sqlx::{Pool, Sqlite};
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::server::Reply;

pub(crate) async fn serve_synchronise_ticket(config: Config,
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
    let err_msg = format!("{request_id} ERROR invalid parameters. SYNCHRONISE_TICKET needs one parameter (a jira issue like PROJ-123) but got {nr_params} instead. Params=[{params}]\n");
    let _ = out_for_replies.send(Reply(err_msg)).await;
  } else {
    let issue_key = splitted_params[0];

    let mut db_conn = db_conn;
    update_interesting_projects_in_db(&config, &mut db_conn).await;
    add_details_to_issue_in_db(&config, issue_key, &mut db_conn).await;
  }

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}
