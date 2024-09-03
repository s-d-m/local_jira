use std::collections::HashMap;
use base64::Engine;
use sqlx::{Error, FromRow, Pool, Sqlite};
use sqlx::types::JsonValue;
use crate::atlassian_document_format::root_elt_doc_to_string;
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::server::Reply;

#[derive(FromRow, Debug)]
struct Relations {
  link_name: String,
  other_issue_key: String,
  other_issue_summary: Option<String>,
}

#[derive(FromRow, Debug)]
struct Field {
  name: String,
  value: JsonValue,
  schema: JsonValue,
}

#[derive(FromRow, Debug)]
struct Comment {
  data: JsonValue,
  author: String,
  creation_time: String,
  last_modification: String
}


async fn get_fields(jira_key: &str, is_custom: bool, db_conn: &Pool<Sqlite>) -> Result<Vec<Field>, sqlx::error::Error> {
  let query_str =
    "SELECT DISTINCT Field.human_name AS name, field_value AS value, schema
      FROM Field
      JOIN IssueField ON IssueField.field_id == Field.jira_id
      JOIN Issue ON Issue.jira_id == IssueField.issue_id
      WHERE Issue.key == ?
        AND is_custom == ?
      ORDER BY name ASC";
  let is_custom_as_int = if is_custom { 1 } else { 0 };
  sqlx::query_as::<_, Field>(query_str)
    .bind(jira_key)
    .bind(is_custom_as_int)
    .fetch_all(db_conn)
    .await
}


async fn get_jira_ticket_as_markdown(jira_key: &str, db_conn: &Pool<Sqlite>) -> Result<String, String> {
  // query returns {jira_key} is satisfied by {all these other keys}
  let inward_relations_query = "
    SELECT DISTINCT IssueLinkType.inward_name AS link_name, Issue.key as other_issue_key, IssueField.field_value AS other_issue_summary
    FROM Issue
    JOIN IssueLink ON IssueLink.inward_issue_id = Issue.jira_id
    JOIN IssueLinkType ON IssueLinkType.jira_id = IssueLink.link_type_id
    JOIN IssueField ON IssueField.issue_id = IssueLink.inward_issue_id
    WHERE IssueLink.outward_issue_id =  (SELECT jira_id FROM Issue WHERE Issue.key == ?)
    AND IssueField.field_id == 'summary'
    ORDER BY link_name ASC,
             Issue.jira_id ASC;";

  // query return {jira_key} satisfies {all there other keys}
  let outward_relations_query = "
    SELECT DISTINCT IssueLinkType.outward_name AS link_name, Issue.key AS other_issue_key, IssueField.field_value AS other_issue_summary
    FROM Issue
    JOIN IssueLink ON IssueLink.outward_issue_id = Issue.jira_id
    JOIN IssueLinkType ON IssueLinkType.jira_id = IssueLink.link_type_id
    JOIN IssueField ON IssueField.issue_id = IssueLink.outward_issue_id
    WHERE IssueLink.inward_issue_id =  (SELECT jira_id FROM Issue WHERE Issue.key == ?)
    AND IssueField.field_id = 'summary'
    ORDER BY link_name ASC,
             Issue.jira_id ASC;";

  let outward_links = sqlx::query_as::<_, Relations>(outward_relations_query)
    .bind(jira_key)
    .fetch_all(db_conn)
    .await;

  let inward_links = sqlx::query_as::<_, Relations>(inward_relations_query)
    .bind(jira_key)
    .fetch_all(db_conn)
    .await;

  let outward_links = match outward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };

  let inward_links = match inward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };


  let custom_fields = get_fields(jira_key, true, db_conn).await;
  let custom_fields = match custom_fields {
    Ok(v) => { v }
    Err(e) => { return Err(format!("Error retrieving custom fields of ticket {jira_key}: {e:?}")) },
  };

  let system_fields = get_fields(jira_key, false, db_conn).await;
  let system_fields = match system_fields {
    Ok(v) => { v }
    Err(e) => {
      return Err(format!("Error retrieving system fields of ticket {jira_key}: {e:?}"))
    }
  };

  let hashed_system_fields = system_fields
    .iter()
    .map(|x| (x.name.as_str(), x))
    .collect::<HashMap<_, &Field>>();

  let summary = system_fields
    .iter()
    .find(|x| x.name == "Summary")
    .and_then(|x| x.value.as_str());

  let Some(summary) = summary else {
    return Err(format!("Error retrieving the summary for ticket {jira_key}"))
  };

  let description = hashed_system_fields.get("Description")
    .and_then(|x| Some(root_elt_doc_to_string(&x.value)));
  let Some(description) = description else {
    return Err(format!("Error retrieving the description for ticket {jira_key}"))
  };

  let links_str = outward_links
    .iter()
    .chain(&inward_links)
    .map(|x| {
      let relation = x.link_name.as_str();
      let other_key = x.other_issue_key.as_str();
      let summary = match &x.other_issue_summary {
        None => { "" }
        Some(a) => { a.as_str() }
      };
      format!("{relation} {other_key}: {summary}")
    })
    .reduce(|a, b| { format!("{a}\n{b}")})
    .unwrap_or(String::from("No link to other issues found"));

  let comments_query_str =
    "SELECT content_data as data, displayName as author, creation_time, last_modification_time as last_modification
     FROM Comment
     JOIN People
       ON People.accountId = Comment.author
     Where issue_id = (SELECT jira_id from Issue WHERE key = ?)
     ORDER BY position_in_array ASC";

  let comments = sqlx::query_as::<_, Comment>(comments_query_str)
    .bind(jira_key)
    .fetch_all(db_conn)
    .await;
  let comments = match comments {
    Ok(v) => {v}
    Err(e) => {
      return Err(format!("Error retrieving custom fields of ticket {jira_key}: {e:?}"))
    }
  };

  let comments = comments
    .iter()
    .map(|x| {
      let author = &x.author;
      let creation = &x.creation_time;
      let last_modification = &x.last_modification;
      let data = root_elt_doc_to_string(&x.data);
      format!("comment from: {author}
last edited on: {last_modification}
{data}")
    })
    .reduce(|a, b| format!("{a}\n\n{b}"))
    .unwrap_or(String::from("no comment found"));

  let res = format!(
    "{jira_key}: {summary}
=========

Description:
----
{description}

Links:
----
{links_str}

Comments:
-----
{comments}
");

  Ok(res)
}

enum output_format {
  MARKDOWN,
  HTML,
  UNKNOWN(String)
}

impl output_format {
  fn new(format: &str) -> Self {
    match format {
      "MARKDOWN" => output_format::MARKDOWN,
      "HTML" => output_format::HTML,
      _ => output_format::UNKNOWN(format.to_string())
    }
  }
}

async fn get_jira_ticket(format: &output_format, issue_key: &str, db_conn: &Pool<Sqlite>) -> Result<String, String> {
  match format {
    output_format::MARKDOWN => {
      get_jira_ticket_as_markdown(issue_key, db_conn).await
    }
    output_format::HTML => { Err(String::from("HTML output is not implemented yet")) }
    output_format::UNKNOWN(s) => { Err(format!("Error unknown output format [{s}] to display jira ticket")) }
  }
}

pub(crate) async fn serve_fetch_ticket_request(config: Config,
                                               request_id: &str,
                                               params: &str,
                                               out_for_replies: tokio::sync::mpsc::Sender<Reply>, db_conn: &mut Pool<Sqlite>) {
  let _ = out_for_replies.send(Reply(format!("{request_id} ACK\n"))).await;

  let splitted_params = params
    .split(',')
    .collect::<Vec<_>>();

  let nr_params = splitted_params.len();
  if nr_params != 2 {
    let err_msg = format!("{request_id} ERROR invalid parameters. FETCH_TICKET needs two parameters separated by commas but got {nr_params} instead. Params=[{params}]\n");
    let _ = out_for_replies.send(Reply(err_msg)).await;
  } else {
    let issue_key = splitted_params[0];
    let format = splitted_params[1];

    let format = output_format::new(format);

    let new_data = get_jira_ticket(&format, issue_key, db_conn).await;
    match &new_data {
      Ok(data) => {
        let data = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
      }
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }

    let mut db_conn = db_conn;
    add_details_to_issue_in_db(&config, issue_key, &mut db_conn).await;
    // Todo: instead of keeping the old data in order to check for differences, only keep a hash
    // this will use fewer data at a time and make the comparison to find out if something
    // changed or not faster.
    //
    // Todo: Also update the links between tickets.
    let new_data = get_jira_ticket(&format, issue_key, db_conn).await;
    match (&new_data, &new_data) {
      (Ok(new_data), Ok(old_data)) if new_data == old_data => {},
      (Ok(new_data), _) => {
        let data = base64::engine::general_purpose::STANDARD.encode(new_data.as_bytes());
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
      },
      (Err(e), _) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }

    update_interesting_projects_in_db(&config, db_conn).await;
    let newest_data = get_jira_ticket(&format, issue_key, db_conn).await;
    match (&newest_data, &new_data) {
      (Ok(newest_data), Ok(new_data)) if newest_data == new_data => {},
      (Ok(newest_data), _) => {
        let data = base64::engine::general_purpose::STANDARD.encode(newest_data.as_bytes());
        let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
      },
      (Err(e), _) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
      }
    }
  }

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}
