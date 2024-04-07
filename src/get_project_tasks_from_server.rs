use sqlx::types::JsonValue;
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;

pub(crate) async fn get_project_tasks_from_server(project_key: &str, config: &Config) -> Result<JsonValue, String> {
    let query = format!("/rest/api/3/search?jql=project%3D%22{project_key}%22+ORDER+BY+created+DESC&startAt=0&maxResults=100&expand=names");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
        return Err(format!("Error: failed to get tasks of project {project_key} from server.\n{e}", e=json_data.err().unwrap().to_string()));
    };
    Ok(json_data)
}