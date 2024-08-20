use std::collections::HashMap;
use std::io;
use serde_json::json;

use sqlx::{FromRow, Pool, Sqlite};
use sqlx::types::JsonValue;

use crate::atlassian_document_format::root_elt_doc_to_string;

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
  last_modification: String,
}

async fn get_fields(jira_key: &str, is_custom: bool, db_conn: &Pool<Sqlite>) -> Result<Vec<Field>, sqlx::error::Error> {
  let query_str =
    "SELECT DISTINCT Field.human_name AS name, field_value AS value, schema
      FROM Field
      JOIN IssueField ON IssueField.field_id == Field.jira_id
      JOIN Issue ON Issue.jira_id == IssueField.issue_id
      WHERE Issue.key == ?
        AND is_custom == ?
      ORDER BY name";
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
    Ok(e) => {e}
    Err(e) => { return Err(e.to_string())}
  };

  let inward_links = match inward_links {
    Ok(e) => {e}
    Err(e) => { return Err(e.to_string())}
  };


  let custom_fields = get_fields(jira_key, true, db_conn).await;
  let custom_fields = custom_fields.unwrap_or_else(|x| {
    eprintln!("Error retrieving custom fields of ticket {jira_key}: {x:?}");
    vec![]
  });

  let system_fields = get_fields(jira_key, false, db_conn).await;
  let system_fields = system_fields.unwrap_or_else(|x| {
    eprintln!("Error retrieving system fields of ticket {jira_key}: {x:?}");
    vec![]
  });
  let hashed_system_fields = system_fields
    .iter()
    .map(|x| (x.name.as_str(), x))
    .collect::<HashMap<_, &Field>>();

  //dbg!(&custom_fields);

  let summary = system_fields
    .iter()
    .find(|x| x.name == "Summary")
    .and_then(|x| x.value.as_str())
    .unwrap_or_default();

  let description = hashed_system_fields.get("Description")
    .and_then(|x| Some(root_elt_doc_to_string(&x.value)))
    .unwrap_or_default();


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
    .unwrap_or_default();

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
  let comments = comments.unwrap_or_else(|x| {
    eprintln!("Error retrieving custom fields of ticket {jira_key}: {x:?}");
    vec![]
  });

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
    .unwrap_or_default();

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


async fn serve_request(request: &str, db_conn: &Pool<Sqlite>) -> Result<String, String> {
  let valid_request_format = "<token><space>GET_JIRA<space><jira_id/key><space>FORMAT<space><json|html|markdown>";

  let split_request = request.split_whitespace().collect::<Vec<_>>();
  if (split_request.len() != 5) || (split_request[1] != "GET_JIRA") || (split_request[3] != "FORMAT")
    || ((split_request[4] != "json") && (split_request[4] != "html") && (split_request[4] != "markdown")) {
    return Err(format!("invalid request. Got [{request}] expecting something like [{valid_request_format}]"));
  }

  let token = split_request[0];
  let jira_id = split_request[2];
  let format = split_request[4];

  let res = get_jira_ticket_as_markdown(jira_id, &db_conn).await;
  return Ok(format!("Ok: token=[{token}] jira_id=[{jira_id}] format=[{format}], res=[{res:?}]"));
}

pub(crate)
async fn server_request_loop(db_conn: &Pool<Sqlite>) {
  eprintln!("Ready to accept requests");
  let mut line: String = Default::default();
  'main_loop: loop {
    line.clear();
    match io::stdin().read_line(&mut line) {
      Ok(_) => {
        let line = line.trim_end();
        eprintln!("Received request [{line}]");
        match line {
          "quit" | "exit" => { break 'main_loop; }
          _ => {
            let res = serve_request(line, db_conn).await;
            let res = match res {
              Ok(e) => { format!("OK: {e}") }
              Err(e) => { format!("ERR:{e}") }
            };
            println!("{res}")
          }
        }
      }
      Err(e) => { eprintln!("Failed to read request. Err: {e:?}") }
    }
  }
}