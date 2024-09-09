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

    // todo: implement quick exit here. We update all tickets because if links are added to a newly created
    // ticket, adding that link to the database would fail due to foreign key constraint.
    // Therefore, to ensure we get all latest data, we update all tickets first, such that
    // creating new links will be guaranteed to work.
    // Most of the time this is unnecessary since links don't change often. And even less so
    // to newly created tickets. In other words, we could check if the links are up to date
    // separately first, and if they are, we can skip the part about updating interesting
    // project. We could still do it, but in the background, after replying that we finished
    // this request. From a user point of view, this request is finished when the given
    // ticket is guaranteed to be up to date.
    let mut db_conn = db_conn;
    update_interesting_projects_in_db(&config, &mut db_conn).await;

    //Ideally we would simply call add_details_to_issue_in_db, but the function update_interesting_projects_in_db
    // relies on tickets not being updated alone in order to find out which ticket to update and which not.
    // when running the synchronise_updated request.
    //
    //    add_details_to_issue_in_db(&config, issue_key, &mut db_conn).await;
  }

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}
