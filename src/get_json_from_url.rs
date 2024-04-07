use base64::Engine;
use sqlx::types::JsonValue;
use crate::get_config::Config;

pub(crate) async fn get_json_from_url(conf: &Config, get_part: &str) -> Result<JsonValue, String> {
    let url = format!("{server}/{query}", server = conf.server_address(), query = get_part);
    let auth_token = conf.auth_token();

    let client = reqwest::Client::new();
    let response = client.get(url.as_str())
        .header("Authorization", format!("Basic {auth_token}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send()
        .await;

    let Ok(response) = response else {
        return Err(format!("Error: failed to get projects. Msg={e}", e = response.err().unwrap().to_string()));
    };

    let Ok(text) = response.text().await else {
        return Err("Error: failed to get text out of response".to_string());
    };

    let json_data = serde_json::from_str::<serde_json::Value>(text.as_str());
    match json_data {
        Ok(v) => Ok(v),
        Err(e) => Err(format!("Error: Failed to parse response as json. Text is [{e}]")),
    }
}