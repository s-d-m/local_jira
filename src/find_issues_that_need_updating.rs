use crate::find_issues_that_need_updating::FoundIssueUpToDate::ONE_ISSUE_IS_UP_TO_DATE;
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_issuelinktype_table::IssueLinkType;
use serde_json::Value;
use sqlx::types::JsonValue;
use sqlx::{FromRow, Pool, Sqlite};

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

// returns a list of issues plus a boolean indicating if there might be more issues to update.
// In other words, the boolean tells if it found an issue which is already up-to-date.
// in such case, since loading must be done from oldest to newest, any further issue should
// normally be up-to-date already
#[derive(Debug)]
pub struct issue_data {
    pub id: i64,
    pub jira_issue: String,
    pub last_updated: String,
}

#[derive(PartialEq, Debug)]
enum FoundIssueUpToDate {
    NO_ISSUE_IS_UP_TO_DATE,
    ONE_ISSUE_IS_UP_TO_DATE,
}

async fn get_issues_from_json_that_need_updating(
    json_data: &Value,
    db_conn: &Pool<Sqlite>,
) -> Result<(Vec<issue_data>, FoundIssueUpToDate), String> {
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
            let id = x.get("id").and_then(|x| x.as_str());
            let id = id.and_then(|x| x.parse::<i64>().ok());
            let jira_issue = x.get("key").and_then(|x| x.as_str());
            let last_updated = x
                .get("fields")
                .and_then(|x| x.as_object())
                .and_then(|x| x.get("updated"))
                .and_then(|x| x.as_str());

            let Some(id) = id else {
                eprintln!("received json data didn't have an id");
                return None;
            };

            let Some(jira_issue) = jira_issue else {
                eprintln!("received json data didn't have an issue number");
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
                let timestamp_to_use = if data.timestamp.starts_with('"') {
                    &data.timestamp[1..]
                } else {
                    &data.timestamp
                };
                let timestamp_to_use = if timestamp_to_use.ends_with('"') {
                    let end = timestamp_to_use.len() - 1;
                    &timestamp_to_use[0..end]
                } else {
                    timestamp_to_use
                };
                eprintln!(
                    "Comparing\n{a}\n{b}\n\n",
                    a = timestamp_to_use,
                    b = issue.last_updated
                );
                if timestamp_to_use == issue.last_updated {
                    return Ok((
                        issues_to_update,
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
    Ok((issues_to_update, FoundIssueUpToDate::NO_ISSUE_IS_UP_TO_DATE))
}

// this here assumes that we load new tickets or update existing ones from oldest to newest.
// Consequently, if PROJ-AAA is in database and up to date, so shall be PROJ-BBB
// for all BBB that are lower than AAA. The consequence here is that we can stop looking
// for tickets which are out of date as soon as we find out where its update time
// on the server matches its update time field on the local database
pub(crate) async fn get_issues_that_need_updating(
    project_key: &str,
    config: &Config,
    db_conn: &Pool<Sqlite>,
) -> Result<Vec<issue_data>, String> {
    eprintln!(
        "Querying issues/tasks for project {project_key} in search of tickets that need updating"
    );
    let max_result_per_query = -1; // -1 is a special value telling jira "no limit"
                                   // the returned json will tell us what is the configured limit
    let first_json = get_one_json(project_key, config, 0, max_result_per_query).await;
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
        get_issues_from_json_that_need_updating(&first_json, db_conn).await;
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
                    get_issues_from_json_that_need_updating(&next_json, db_conn).await;
                match new_issues_to_update {
                    Ok(mut v) => {
                        res.append(&mut v.0);
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
