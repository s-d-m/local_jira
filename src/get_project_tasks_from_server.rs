use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use serde_json::{Map, Value};
use sqlx::types::JsonValue;

async fn get_one_json(
    project_key: &str,
    config: &Config,
    start: i64,
    max_result_per_query: i32,
) -> Result<JsonValue, String> {
    let query = format!("/rest/api/3/search?jql=project%3D%22{project_key}%22+ORDER+BY+created+ASC&startAt={start}&maxResults={max_result_per_query}&expand=names");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get tasks of project {project_key} from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };
    Ok(json_data)
}

pub(crate) async fn get_project_tasks_from_server(
    project_key: &str,
    config: &Config,
) -> Result<Vec<JsonValue>, String> {
    eprintln!("Querying issues/tasks for project {project_key}");
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

    let mut res: Vec<_> = vec![first_json];

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
            Ok(e) => res.push(e),
            Err(e) => {
                return Err(e);
            }
        }
    }

    Ok(res)
}
