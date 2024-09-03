use std::cmp::Ordering::{Equal, Greater, Less};
use crate::find_issues_that_need_updating::FoundIssueUpToDate::ONE_ISSUE_IS_UP_TO_DATE;
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_issuelinktype_table::IssueLinkType;
use serde_json::Value;
use sqlx::types::JsonValue;
use sqlx::{FromRow, Pool, Sqlite};
use tokio::task::JoinSet;
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::get_project_tasks_from_server::get_project_tasks_from_server;
use crate::manage_interesting_projects::{get_issue_links_from_json, Issue, IssueLink, update_issue_links_in_db, update_issues_in_db};
use crate::manage_issue_field::{fill_issues_fields, fill_issues_fields_from_json, IssueProperties, KeyValueProperty};
use crate::utils::get_str_without_surrounding_quotes;

async fn get_one_json(
    project_key: &str,
    config: &Config,
    start: i64,
    max_result_per_query: i32,
) -> Result<JsonValue, String> {
    let query = format!("/rest/api/3/search?jql=project%3D%22{project_key}%22+ORDER+BY+updated+DESC&startAt={start}&maxResults={max_result_per_query}");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get tasks of project {project_key} from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };
    Ok(json_data)
}

#[derive(Debug)]
pub struct issue_data {
    pub id: i64,
    pub jira_issue: String,
    pub last_updated: String,
    pub fields: serde_json::map::Map<String, serde_json::Value>
}

#[derive(Debug)]
pub struct issue_and_links {
    issues: Vec<issue_data>,
    links: Vec<IssueLink>,
}

#[derive(PartialEq, Debug)]
enum FoundIssueUpToDate {
    NO_ISSUE_IS_UP_TO_DATE,
    ONE_ISSUE_IS_UP_TO_DATE,
}

// returns a list of issues and links plus a boolean indicating if there might be more issues to update.
// In other words, the boolean tells if it found an issue which is already up-to-date.
// in such case, since loading must be done from oldest to newest, any further issue should
// normally be up-to-date already
async fn get_issues_and_link_from_json_that_need_updating(
    json_data: &Value,
    db_conn: &Pool<Sqlite>,
) -> Result<(issue_and_links, FoundIssueUpToDate), String> {
    let links = get_issue_links_from_json(json_data);
    let links = match links {
        Ok(v) => {v}
        Err(e) => {
            eprintln!("Error while trying to retrieve links for json: Err {e}");
            vec![]
        }
    };

    let json_data = json_data
        .as_object()
        .and_then(|x| x.get("issues"))
        .and_then(|x| x.as_array());

    let Some(json_data) = json_data else {
        let err_msg =
            "No issue was found in the json passed to get_issues_from_json_that_need_updating";
        eprintln!("{err_msg}");
        return Err(String::from(err_msg));
    };

    let issues_to_check = json_data
        .iter()
        .filter_map(|x| x.as_object())
        .filter_map(|x| {
            let id = x
              .get("id")
              .and_then(|x| x.as_str());
            let id = id
              .and_then(|x| x.parse::<i64>().ok());
            let jira_issue = x
              .get("key")
              .and_then(|x| x.as_str());
            let issue_fields = x
              .get("fields")
              .and_then(|x| x.as_object());

            let last_updated = issue_fields
                .and_then(|x| x.get("updated"))
                .and_then(|x| x.as_str());

            let Some(id) = id else {
                eprintln!("received json data didn't have an id");
                return None;
            };

            let Some(jira_issue) = jira_issue else {
                eprintln!("received json data didn't have an issue number (id: {id})");
                return None;
            };

            let Some(issue_fields) = issue_fields else {
                eprintln!("received json data for issue {jira_issue} (id: {id}) doesn't contain fields");
                return None;
            };

            let Some(last_updated) = last_updated else {
                eprintln!("received json data didn't have a last modification number");
                return None;
            };

            Some(issue_data {
                id,
                jira_issue: jira_issue.to_string(),
                last_updated: last_updated.to_string(),
                fields: issue_fields.clone(),
            })
        })
        .collect::<Vec<_>>();


    #[derive(FromRow, Debug)]
    struct LastModified {
        timestamp: String,
    }

    let mut issues_to_update = vec![];

    let query_str = "SELECT field_value as timestamp
     FROM IssueField
     WHERE field_id = 'updated'
      AND (issue_id = ?);";
    for issue in issues_to_check {
        let cur_id = issue.id;
        eprintln!("Checking if issue with id {cur_id} is up to date");
        let row = sqlx::query_as::<_, LastModified>(query_str)
            .bind(cur_id)
            .fetch_optional(db_conn)
            .await;

        match row {
            Ok(Some(data)) => {
                let timestamp_to_use = get_str_without_surrounding_quotes(data.timestamp.as_str());
                eprintln!(
                    "Comparing\n{a}\n{b}\n\n",
                    a = timestamp_to_use,
                    b = issue.last_updated
                );
                if timestamp_to_use == issue.last_updated {
                    return Ok((
                        issue_and_links {
                            issues: issues_to_update,
                            links,
                        },

                        FoundIssueUpToDate::ONE_ISSUE_IS_UP_TO_DATE,
                    ));
                }
                issues_to_update.push(issue)
            }
            Ok(None) => {
                eprintln!("Ticket is not in database yet");
                issues_to_update.push(issue)
            }
            Err(e) => {
                eprintln!("Error occurred while trying to get the last modification time for issue with id: {cur_id}. Err={e:?}");
            }
        }
    }
    Ok((issue_and_links {
        issues: issues_to_update,
        links,
    }, FoundIssueUpToDate::NO_ISSUE_IS_UP_TO_DATE))
}

// this here assumes that we load new tickets or update existing ones from oldest to newest.
// Consequently, if PROJ-AAA is in database and up to date, so shall be PROJ-BBB
// for all BBB that are lower than AAA. The consequence here is that we can stop looking
// for tickets which are out of date as soon as we find out where its update time
// on the server matches its update time field on the local database
async fn get_issues_and_links_that_need_updating(
    project_key: &str,
    config: &Config,
    db_conn: &Pool<Sqlite>,
) -> Result<issue_and_links, String> {
    eprintln!(
        "Querying issues/tasks for project {project_key} in search of tickets that need updating"
    );
    let max_result_per_query = -1; // -1 is a special value telling jira "no limit"
                                   // the returned json will tell us what is the configured limit
    let first_json = get_one_json(project_key, &config, 0, max_result_per_query).await;
    let Ok(first_json) = first_json else {
        return Err(first_json.err().unwrap());
    };

    let max_result_per_query = first_json
        .as_object()
        .and_then(|x| x.get("maxResults"))
        .and_then(|x| x.as_i64())
        .unwrap_or_else(|| {
            eprintln!(
                "Couldn't retrieve the number of max results from the jira server. Using 100"
            );
            100
        });

    let total = first_json
        .as_object()
        .and_then(|x| x.get("total"))
        .and_then(|x| x.as_i64());

    let first_issues_to_update =
        get_issues_and_link_from_json_that_need_updating(&first_json, db_conn).await;
    let first_issues_to_update = match first_issues_to_update {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Err: {e:?}");
            return Err(e);
        }
    };
    if first_issues_to_update.1 == ONE_ISSUE_IS_UP_TO_DATE {
        return Ok(first_issues_to_update.0);
    }

    let mut res = first_issues_to_update.0;

    let Some(total) = total else {
        return Ok(res);
    };

    if total <= max_result_per_query {
        return Ok(res);
    }

    for i in 0..(total / max_result_per_query) {
        let start = max_result_per_query * (i + 1);
        eprintln!(
            "Querying issues/tasks starting from {start} out of {total} for project {project_key}"
        );
        let next_json = get_one_json(project_key, config, start, max_result_per_query as i32).await;
        match next_json {
            Ok(next_json) => {
                let new_issues_to_update =
                    get_issues_and_link_from_json_that_need_updating(&next_json, db_conn).await;
                match new_issues_to_update {
                    Ok(mut v) => {
                        let mut issues_and_links_from_this_json = v.0;
                        res.links.append(&mut issues_and_links_from_this_json.links);
                        res.issues.append(&mut issues_and_links_from_this_json.issues);

                        if v.1 == ONE_ISSUE_IS_UP_TO_DATE {
                            return Ok(res);
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            Err(e) => {
                return Err(e);
            }
        }
    }

    Ok(res)
}


async fn update_given_project_in_db(config: Config, project_key: String, mut db_conn: Pool<Sqlite>) {
    let issues_and_links_to_update = get_issues_and_links_that_need_updating(project_key.as_str(), &config, &db_conn).await;
    let mut db_handle = db_conn.clone();

    if let Ok(issues_and_links_to_update) = issues_and_links_to_update {
        // First insert all issues in the db, and then insert the links between issues.
        // This avoids the issues where inserting links fails due to foreign constraints violation
        // at the database layer because some issues are linked to others which crosses a pagination
        // limit.
        let issues_to_upsert = issues_and_links_to_update.issues
          .iter()
          .map(|x| {
              let issue_id = x.id as u32;
              Issue{
                  jira_id: issue_id,
                  key: x.jira_issue.clone(),
                  project_key: project_key.clone(),
              }
          })
          .collect::<Vec<_>>();

        update_issues_in_db(&issues_to_upsert, &mut db_conn, project_key.as_str()).await;

        let mut fields_to_upsert = issues_and_links_to_update.issues
          .iter()
          .map(|x| {
              let issue_id = x.id as u32;
              let properties = x.fields
                .iter()
                .map(|(key, value)| KeyValueProperty {
                    key: key.to_string(),
                    value: value.to_string(),
                })
                .collect::<Vec<_>>();

              IssueProperties { issue_id, properties }
          })
          .collect::<Vec<_>>();

        fields_to_upsert.sort_by(|a, b| match (a.issue_id, b.issue_id) {
            (a, b) if a < b => { Less }
            (a, b) if a == b => { Equal }
            (a, b) if a > b => { Greater }
            (_, _) => panic!()
        });

        fill_issues_fields(&fields_to_upsert, &mut db_conn).await;

        // now insert the links
        let issue_ids = issues_to_upsert
          .iter()
          .map(|x| x.jira_id)
          .collect::<Vec<_>>();
        let issue_links = issues_and_links_to_update.links;
        update_issue_links_in_db(issue_ids.as_slice(), &issue_links, &mut db_conn).await;


        // now get the full data for each issue.
        let issues_keys = issues_to_upsert
          .iter()
          .map(|x| x.project_key.as_str())
          .collect::<Vec<_>>();

        for key in issues_keys {
            add_details_to_issue_in_db(&config,
                                       key,
                                       &mut db_conn).await
        }
    }
}

pub(crate) async fn update_interesting_projects_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
    let interesting_projects = config.interesting_projects();

    let mut tasks = interesting_projects
      .iter()
      .map(|x| tokio::spawn(update_given_project_in_db(config.clone(), x.clone(), db_conn.clone())))
      .collect::<JoinSet<_>>();

    while let Some(res) = tasks.join_next().await {
    }
}
