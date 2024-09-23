use std::collections::HashMap;
use base64::Engine;
use serde_json::{json, Map, Value};
use sqlx::{Error, FromRow, Pool, Sqlite};
use sqlx::types::JsonValue;
use crate::atlassian_document_format::root_elt_doc_to_string;
use crate::atlassian_document_format_html_output::root_elt_doc_to_html_string;
use crate::atlassian_document_utils::indent_with;
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::get_issue_details::{add_details_to_issue_in_db, get_json_for_issue};
use crate::manage_field_table::get_fields_from_database;
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
}

#[derive(FromRow, Debug)]
struct Comment {
  data: JsonValue,
  author: String,
  creation_time: String,
  last_modification: String
}


async fn get_fields_from_db(jira_key: &str, is_custom: bool, db_conn: &Pool<Sqlite>) -> Result<Vec<Field>, sqlx::error::Error> {
  let query_str =
    "SELECT DISTINCT Field.human_name AS name, field_value AS value
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

async fn get_inward_links_from_db(jira_key: &str, db_conn: &Pool<Sqlite>) -> Result<Vec<Relations>, Error> {
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

  let inward_links = sqlx::query_as::<_, Relations>(inward_relations_query)
    .bind(jira_key)
    .fetch_all(db_conn)
    .await;

  inward_links
}

async fn get_outward_links_from_db(jira_key: &str, db_conn: &Pool<Sqlite>) -> Result<Vec<Relations>, Error> {
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

  outward_links
}

async fn get_comments_from_db(jira_key: &str, db_conn: &Pool<Sqlite>) -> Result<Vec<Comment>, Error> {
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
  comments
}

fn format_links_for_html(inward_links: &[Relations], outward_links: &[Relations]) -> String {
  let links_str = outward_links
    .iter()
    .chain(inward_links.iter())
    .map(|x| {
      let relation = x.link_name.as_str();
      let other_key = x.other_issue_key.as_str();
      let summary = match &x.other_issue_summary {
        None => { "" }
        Some(a) => { a.as_str() }
      };
      let relation = html_escape::encode_safe(relation);
      let other_key = html_escape::encode_safe(other_key);
      let summary = html_escape::encode_safe(summary);
      format!(
"<div class=\"link\">
  <div class=\"relation\">{relation}</div>
  <div class=\"other_key\">{other_key}</div>
  <div class=\"key_summary\">{summary}</div>
</div>")
    })
    .reduce(|a, b| { format!("{a}\n{b}")})
    .unwrap_or(String::from("No link to other issues found"));

  links_str
}

fn format_comments_for_html(comments: &[Comment], db_conn: &Pool<Sqlite>) -> String {
  let comments = comments
    .iter()
    .map(|x| {
      let author = &x.author;
      let creation = &x.creation_time;
      let last_modification = &x.last_modification;
      let data = root_elt_doc_to_html_string(&x.data, &db_conn);
      let author = html_escape::encode_safe(author);
      let creation = html_escape::encode_safe(creation);
      let last_modification = html_escape::encode_safe(last_modification);
      format!(
"<div class=\"comment\">
  <div class=\"comment_author\">comment author: {author}</div>
  <div class=\"comment_last_edited\">last edited on: {last_modification}</div>
  <div class=\"comment_message\">{data}</div>
</div>")
    })
    .reduce(|a, b| format!("{a}\n{b}"))
    .unwrap_or(String::from("no comment found"));

  comments
}

fn get_summary<'a>(hashed_system_fields: &HashMap<&str, &'a Field>) -> &'a str {
  let summary = hashed_system_fields.get("Summary")
    .and_then(|x| x.value.as_str())
    .unwrap_or("no summary provided");

  summary
}

fn get_html_summary<'a>(hashed_system_fields: &HashMap<&str, &'a Field>) -> std::borrow::Cow<'a, str> {
  let summary = get_summary(hashed_system_fields);
  let summary = html_escape::encode_safe(summary);
  summary
}

fn get_markdown_description(hashed_system_fields: &HashMap<&str, &Field>) -> String {
  let description = hashed_system_fields.get("Description")
    .and_then(|x| Some(root_elt_doc_to_string(&x.value)))
    .unwrap_or(String::from("no description provided"));

  description
}

fn get_html_description(hashed_system_fields: &HashMap<&str, &Field>, db_conn: &Pool<Sqlite>) -> String {
  let description = hashed_system_fields.get("Description")
    .and_then(|x| Some(root_elt_doc_to_html_string(&x.value, &db_conn)))
    .unwrap_or(String::from("no description provided"));

  description
}

fn format_ticket_for_html(issue_key: &str,
                          system_fields: &[Field],
                          custom_fields: &[Field],
                          inward_links: &[Relations],
                          outward_links: &[Relations],
                          comments: &[Comment],
                          db_conn: &Pool<Sqlite>) -> Result<String, String> {

  let hashed_system_fields = system_fields
    .iter()
    .map(|x| (x.name.as_str(), x))
    .collect::<HashMap<_, &Field>>();

  let summary = get_html_summary(&hashed_system_fields);
  let description = get_html_description(&hashed_system_fields, db_conn);

  let links_str = format_links_for_html(inward_links.as_ref(),
                                        outward_links.as_ref());

  let comments = format_comments_for_html(comments.as_ref(),
                                          db_conn);

  let description = indent_with(description.as_str(), "      ");
  let links_str = indent_with(links_str.as_str(), "      ");
  let comments = indent_with(comments.as_str(), "      ");

  let res = format!(
r###"<!DOCTYPE html>
<html lang="en-GB">
  <head>
    <meta charset="UTF-8">
    <title>add a build/test request</title>
<!--    <meta http-equiv="Content-Security-Policy" content="default-src 'unsafe-inline';"> -->
    <link rel="icon" href="data:,">
  </head>
  <body>
    <h1>{issue_key}: {summary}</h1>

    <h2>Description:</h2>
    <div class="description">
{description}
    </div>

    <h2>Links:</h2>
    <div class="links">
{links_str}
    </div>

    <h2>Comments:</h2>
    <div class="comments">
{comments}
    </div>
  </body>
</html>
"###);

  Ok(res)
}

fn format_links_for_markdown(inward_links: &[Relations], outward_links: &[Relations]) -> String {
  let links_str = outward_links
    .iter()
    .chain(inward_links.iter())
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

  links_str
}

fn format_comments_for_markdown(comments: &[Comment]) -> String {
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

  comments
}

fn format_ticket_for_markdown(issue_key: &str,
                              system_fields: &[Field],
                              custom_fields: &[Field],
                              inward_links: &[Relations],
                              outward_links: &[Relations],
                              comments: &[Comment]) -> Result<String, String> {

  let hashed_system_fields = system_fields
    .iter()
    .map(|x| (x.name.as_str(), x))
    .collect::<HashMap<_, &Field>>();

  let summary = get_summary(&hashed_system_fields);
  let description = get_markdown_description(&hashed_system_fields);

  let comments = format_comments_for_markdown(comments.as_ref());
  let links_str = format_links_for_markdown(inward_links.as_ref(), outward_links.as_ref());

  let res = format!(
    "{issue_key}: {summary}
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


#[derive(Clone)]
enum output_format {
  MARKDOWN,
  HTML,
}

impl output_format {
  fn try_new(format: &str) -> Result<Self, String> {
    match format {
      "MARKDOWN" => Ok(output_format::MARKDOWN),
      "HTML" => Ok(output_format::HTML),
      _ => Err(format!("Unknown format for ticket output. Supported: MARKDOWN and HTML. Requested: {format}"))
    }
  }
}

async fn get_jira_ticket_from_db(format: &output_format, issue_key: &str, db_conn: &Pool<Sqlite>) -> Result<String, String> {
  let outward_links = get_outward_links_from_db(issue_key, db_conn);
  let inward_links = get_inward_links_from_db(issue_key, db_conn);

  let outward_links = outward_links.await;
  let inward_links = inward_links.await;

  let outward_links = match outward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };

  let inward_links = match inward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };

  let custom_fields = get_fields_from_db(issue_key, true, db_conn).await;
  let custom_fields = match custom_fields {
    Ok(v) => { v }
    Err(e) => { return Err(format!("Error retrieving custom fields of ticket {issue_key}: {e:?}")) },
  };

  let system_fields = get_fields_from_db(issue_key, false, db_conn).await;
  let system_fields = match system_fields {
    Ok(v) => { v }
    Err(e) => {
      return Err(format!("Error retrieving system fields of ticket {issue_key}: {e:?}"))
    }
  };

  let comments = get_comments_from_db(issue_key, db_conn).await;
  let comments = match comments {
    Ok(v) => {v}
    Err(e) => {
      return Err(format!("Error retrieving custom fields of ticket {issue_key}: {e:?}"))
    }
  };

  let res = format_ticket(
                          issue_key,
                          format,
                          db_conn,
                          outward_links.as_slice(),
                          inward_links.as_slice(),
                          custom_fields.as_slice(),
                          system_fields.as_slice(),
                          comments.as_slice());

  res
}

fn get_inward_links_from_json(json_of_issue: &Map<String, Value>) -> Result<Vec<Relations>, String> {
  let inward_issues = json_of_issue
    .get("fields")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("issuelinks"))
    .and_then(|x| x.as_array());

  // the json file always contains an array of links. It is empty for tickets
  // with no links.
  let inward_issues = match inward_issues {
    None => { return Err(String::from("No issuelinks array found in json when looking for inward issues")) }
    Some(v) => { v }
  };

  let inward_issues = inward_issues
    .into_iter()
    .filter_map(|x| x.as_object())
    .filter(|x| x.get("inwardIssue").is_some())
    .filter_map(|x| {
      let link_name = x
        .get("type")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("inward"))
        .and_then(|x| x.as_str());
      let inward_issue = x
        .get("inwardIssue")
        .and_then(|x| x.as_object());
      let other_issue_key = inward_issue
        .and_then(|x| x.get("key"))
        .and_then(|x| x.as_str());
      let other_issue_summary = inward_issue
        .and_then(|x| x.get("fields"))
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("summary"))
        .and_then(|x| x.as_str());
      match (link_name, other_issue_key, other_issue_summary) {
        (None, _, _) | (_, None, _) => {
          eprintln!("Received a json inward issue that look incomplete");
          None
        },
        (Some(link_name), Some(other_issue_key), other_issue_summary) => {
          let link_name = link_name.to_string();
          let other_issue_key = other_issue_key.to_string();
          let other_issue_summary = other_issue_summary
            .and_then(|x| Some(x.to_string()));
          let relation = Relations {
            link_name,
            other_issue_key,
            other_issue_summary
          };
          Some(relation)
        }
      }
    })
    .collect::<Vec<_>>();
    Ok(inward_issues)
}

struct CustomAndSystemFields {
  custom_fields: Vec<Field>,
  system_fields: Vec<Field>
}

async fn get_fields_from_json(json_of_issue: &Map<String, Value>, db_conn: &Pool<Sqlite>) -> Result<CustomAndSystemFields, String> {
  let fields = json_of_issue
    .get("fields")
    .and_then(|x| x.as_object());

  // the json file always contains an array of fields.
  let fields = match fields {
    None => { return Err(String::from("No fields array found in json when looking for fields in json of an issue")) }
    Some(v) => { v }
  };

  let fields = fields
    .into_iter()
    .filter(|(key, value)| !value.is_null())
    .map(|(key, value)| {
      Field {
        name: String::from(key),
        value: value.to_owned()
      }
    })
    .collect::<Vec<_>>();

  let fields_in_db = get_fields_from_database(db_conn).await;
  let fields_in_db = fields_in_db
    .into_iter()
    .map(|x| (x.jira_id, (x.human_name, x.is_custom)))
    .collect::<HashMap<String, (String, bool)>>();

  let mut custom_fields = Vec::new();
  let mut system_fields = Vec::new();
  for field in fields.into_iter() {
    let field_metadata = fields_in_db.get(&field.name);
    match field_metadata {
      Some((human_name, is_custom)) => {
        let field_with_human_name = Field{name: human_name.to_string(), value: field.value};
        if *is_custom {
          custom_fields.push(field_with_human_name)
        } else {
          system_fields.push(field_with_human_name)
        }
      },
      None => { eprintln!("Error, seems we got a field from remote for which we don't have the proper metadata locally.\
Field key is {x}, value={y}", x=field.name, y=field.value.to_string())}
    }
  }

    let res = CustomAndSystemFields {
      custom_fields,
      system_fields,
    };

    Ok(res)
}

fn get_outward_links_from_json(json_of_issue: &Map<String, Value>) -> Result<Vec<Relations>, String> {
  let outward_issues = json_of_issue
    .get("fields")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("issuelinks"))
    .and_then(|x| x.as_array());

  // the json file always contains an array of links. It is empty for tickets
  // with no links.
  let outward_issues = match outward_issues {
    None => { return Err(String::from("No issuelinks array found in json when looking for outward issues")) }
    Some(v) => { v }
  };

  let outward_issues = outward_issues
    .into_iter()
    .filter_map(|x| x.as_object())
    .filter(|x| x.get("outwardIssue").is_some())
    .filter_map(|x| {
      let link_name = x
        .get("type")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("outward"))
        .and_then(|x| x.as_str());
      let outward_issue = x
        .get("outwardIssue")
        .and_then(|x| x.as_object());
      let other_issue_key = outward_issue
        .and_then(|x| x.get("key"))
        .and_then(|x| x.as_str());
      let other_issue_summary = outward_issue
        .and_then(|x| x.get("fields"))
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("summary"))
        .and_then(|x| x.as_str());
      match (link_name, other_issue_key, other_issue_summary) {
        (None, _, _) | (_, None, _) => {
          eprintln!("Received a json outward issue that look incomplete");
          None
        },
        (Some(link_name), Some(other_issue_key), other_issue_summary) => {
          let link_name = link_name.to_string();
          let other_issue_key = other_issue_key.to_string();
          let other_issue_summary = other_issue_summary
            .and_then(|x| Some(x.to_string()));
          let relation = Relations {
            link_name,
            other_issue_key,
            other_issue_summary
          };
          Some(relation)
        }
      }
    })
    .collect::<Vec<_>>();
  Ok(outward_issues)
}



fn get_comments_from_json(json_of_issue: &Map<String, Value>) -> Result<Vec<Comment>, String> {
  let comments = json_of_issue
    .get("fields")
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("comment"))
    .and_then(|x| x.as_object())
    .and_then(|x| x.get("comments"))
    .and_then(|x| x.as_array());

  let comments = match comments {
    None => {return Err(String::from("couldn't get the comments from the returned json"))}
    Some(v) => {v}
  };

  let comments = comments
    .into_iter()
    .map(|x| { x .as_object() })
    .collect::<Vec<_>>();

  let are_all_objects = comments
    .iter()
    .all(|x| x.is_some());

  if !are_all_objects {
    return Err(String::from("Some data in comments section are not objects"));
  }
  let comments = comments
    .into_iter()
    .filter_map(|x| x)
    .map(|x| {
      let body = x
        .get("body")
        .and_then(|x| x.as_object())
        .and_then(|x| Some(serde_json::value::Value::Object(x.clone())));
      let author = x
        .get("author")
        .and_then(|x| x.as_object())
        .and_then(|x| x.get("displayName"))
        .and_then(|x| x.as_str());
      let creation_time = x
        .get("created")
        .and_then(|x| x.as_str());
      let last_modification = x
        .get("updated")
        .and_then(|x| x.as_str());
      match (body, author, creation_time, last_modification) {
        (Some(body), Some(author), Some(creation_time), Some(last_modification)) => {
          let comment = Comment {
            data: body,
            author: author.to_string(),
            creation_time: creation_time.to_string(),
            last_modification: last_modification.to_string(),
          };
          Some(comment)
        },
        _ => None
      }
    })
    .collect::<Vec<_>>();

  let are_all_comments = comments
    .iter()
    .all(|x| x.is_some());
  if !are_all_comments {
    return Err(String::from("failed to extract some of the comments in the data section"));
  }

  let comments = comments
    .into_iter()
    .filter_map(|x| x)
    .collect::<Vec<_>>();
  Ok(comments)
}

async fn get_jira_ticket_from_remote(format: &output_format, issue_key: &str, config: &Config, db_conn: &Pool<Sqlite>) -> Result<String, String> {
  let json_of_issue = get_json_for_issue(&config, issue_key).await;
  let json_of_issue = match json_of_issue {
    Ok(v) => {v}
    Err(e) => {return Err(e)}
  };

  let json_of_issue = match json_of_issue.as_object() {
    Some(v) => {v}
    None => {return Err(format!("Failed to extract data from json for issue {issue_key}. Json is {json_of_issue:?}"))}
  };

  let outward_links = get_outward_links_from_json(json_of_issue);
  let inward_links = get_inward_links_from_json(json_of_issue);

  let outward_links = match outward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };

  let inward_links = match inward_links {
    Ok(e) => { e }
    Err(e) => { return Err(e.to_string()) }
  };

  let fields = get_fields_from_json(json_of_issue, db_conn).await;
  let fields = match fields {
    Ok(v) => { v }
    Err(e) => { return Err(format!("Error retrieving custom fields of ticket {issue_key}: {e:?}")) },
  };


  let comments = get_comments_from_json(json_of_issue);
  let comments = match comments {
    Ok(v) => {v}
    Err(e) => {
      return Err(format!("Error retrieving custom fields of ticket {issue_key}: {e:?}"))
    }
  };

  let res = format_ticket(issue_key,
                          format,
                          db_conn,
                          outward_links.as_slice(),
                          inward_links.as_slice(),
                          fields.custom_fields.as_slice(),
                          fields.system_fields.as_slice(),
                          comments.as_slice());

  res
}

fn format_ticket(issue_key: &str,
                 format: &output_format,
                 db_conn: &Pool<Sqlite>,
                 outward_links: &[Relations],
                 inward_links: &[Relations],
                 custom_fields: &[Field],
                 system_fields: &[Field],
                 comments: &[Comment]) -> Result<String, String> {
  let res = match format {
    output_format::MARKDOWN => {
      format_ticket_for_markdown(issue_key,
                                 system_fields,
                                 custom_fields,
                                 inward_links,
                                 outward_links,
                                 comments)
    }
    output_format::HTML => {
      format_ticket_for_html(issue_key,
                             system_fields,
                             custom_fields,
                             inward_links,
                             outward_links,
                             comments,
                             db_conn)
    }
  };
  res
}


pub(crate) async fn serve_fetch_ticket_request(config: Config,
                                               request_id: &str,
                                               params: &str,
                                               out_for_replies: tokio::sync::mpsc::Sender<Reply>, db_conn: &mut Pool<Sqlite>) {
  let _ = out_for_replies.send(Reply(format!("{request_id} ACK\n"))).await;

  let splitted_params = params
    .split(',')
    .map(|x| x.to_string())
    .collect::<Vec<_>>();

  let nr_params = splitted_params.len();
  if nr_params != 2 {
    let err_msg = format!("{request_id} ERROR invalid parameters. FETCH_TICKET needs two parameters separated by commas but got {nr_params} instead. Params=[{params}]\n");
    let _ = out_for_replies.send(Reply(err_msg)).await;
  } else {

    let issue_key = &splitted_params[0];
    let format = &splitted_params[1];

    let format = output_format::try_new(format);
    match format {
      Ok(format) => {
        let old_data = get_jira_ticket_from_db(&format, issue_key, db_conn).await;
        match &old_data {
          Ok(data) if data.is_empty() => {
            // shouldn't happen since get_jira_ticket should at least give back the issue id
            // in the reply
            let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
          }
          Ok(data) => {
            let data = base64::engine::general_purpose::STANDARD.encode(data.as_bytes());
            let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
          }
          Err(e) => {
            let _ = out_for_replies.send(Reply(format!("{request_id} ERROR {e}\n"))).await;
          }
        }

        let newest_data = get_jira_ticket_from_remote(&format, issue_key, &config, db_conn).await;
        match (newest_data, old_data) {
          (Ok(newest_data), Ok(old_data)) if newest_data == old_data => {}
          (Ok(newest_data), _) => if newest_data.is_empty() {
            // shouldn't happen since get_jira_ticket should at least give back the issue id
            // in the reply
            let _ = out_for_replies.send(Reply(format!("{request_id} RESULT\n"))).await;
            // todo spawn an update_interesting_projects_in_db in background as we know some data is out of data
          },
          (Ok(newest_data), _) => {
            let data = base64::engine::general_purpose::STANDARD.encode(newest_data.as_bytes());
            let _ = out_for_replies.send(Reply(format!("{request_id} RESULT {data}\n"))).await;
            // todo spawn an update_interesting_projects_in_db in background as we know some data is out of data
          },
          (Err(e), _) => {
            let _ = out_for_replies.send(Reply(format!("{request_id} ERROR failed to get data from remote to see if local data is up to date or note: Err {e:?}\n"))).await;
          }
        };
      },
      Err(e) => {
        let _ = out_for_replies.send(Reply(format!("{request_id} ERROR failed to find a suitable format. Err: {e}\n"))).await;
      }
    }
  }

  let _ = out_for_replies.send(Reply(format!("{request_id} FINISHED\n"))).await;
}
