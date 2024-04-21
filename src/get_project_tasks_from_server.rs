use serde_json::Value;
use sqlx::types::JsonValue;
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;

async fn get_one_json(project_key: &str, config: &Config, start: i64) -> Result<JsonValue, String> {
  let max_result_per_query = 100; // actually hardcoded by jira. Having maxResults = 100000 actually truncates at 100

  let query = format!("/rest/api/3/search?jql=project%3D%22{project_key}%22+ORDER+BY+created+DESC&startAt={start}&maxResults={max_result_per_query}&expand=names");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
      return Err(format!("Error: failed to get tasks of project {project_key} from server.\n{e}", e = json_data.err().unwrap().to_string()));
    };
    Ok(json_data)

}

pub(crate) async fn get_project_tasks_from_server(project_key: &str, config: &Config) -> Result<Vec<JsonValue>, String> {
  let max_result_per_query = 100; // actually hardcoded by jira. Having maxResults = 100000 actually truncates at 100

  eprintln!("Querying issues/tasks for project {project_key}");

  let first_json = get_one_json(project_key, config, 0).await;
  let Ok(first_json) = first_json else {
    return Err(first_json.err().unwrap());
  };

  let total = first_json
    .as_object()
    .and_then(|x| x.get("total"))
    .and_then(|x| x.as_i64());

  let mut res: Vec<_> = vec![first_json];

  match total {
    None => {}
    Some(x) if x <= max_result_per_query => {}
    Some(x) => {
      for i in 0..(x / max_result_per_query) {
        let start = 100 * (i + 1);
        eprintln!("Querying issues/tasks starting from {start} out of {x} for project {project_key}");
        let next_json = get_one_json(project_key, config, start).await;
        match next_json {
          Ok(e) => { res.push(e) }
          Err(e) => { return Err(e); }
        }
      }
    }
  }

  Ok(res)
}