use std::ffi::OsStr;
use std::fmt::format;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use base64::Engine;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Pool, Sqlite, SqlitePool};

use crate::get_config::{Config, get_config};

mod get_config;
mod defaults;


async fn init_db(db_path: &std::path::PathBuf) -> Result<Pool<Sqlite>, String> {
    let path = db_path.to_str();
    let Some(path) = path else { return Err(format!("Unsupported filename [{f}] must be utf8 valid.",
    f = db_path.to_string_lossy())); };
    if !Sqlite::database_exists(path).await.unwrap() {
        println!("Creating database at [{path}]");
        match Sqlite::create_database(path).await {
            Ok(_) => println!("Create db success"),
            Err(error) => return Err(format!("error: {error}")),
        }
    } else {
        println!("Database already exists");
    }

    let db = SqlitePool::connect(path).await.unwrap();
    let create_schema = include_str!("create_schema.sql");
    let result = sqlx::query(create_schema).execute(&db).await.unwrap();
    println!("Create user table result: {:?}", result);
    Ok(db)
}

async fn get_projects(conf: &Config) -> Result<String, String> {
    let url = format!("{server}/{query}", server=conf.server_address(), query="/rest/api/2/project?expand=lead");
    let auth_token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{token}", user=conf.user_login(), token=conf.api_token()).as_str());
    dbg!(&auth_token);

    let client = reqwest::Client::new();
    let response = client.get(url.as_str())
        .header("Authorization", format!("Basic {auth_token}"))
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .send()
        .await;

    let Ok(response) = response else {
        return Err(format!("Error: failed to get projects. Msg={e}", e=response.err().unwrap().to_string()))
    };

    println!("DEBUG: {b}", b=response.status().as_u16());
    dbg!(&response);

    let Ok(text) = response.text().await else {
        return Err("Error: failed to get text out of response".to_string());
    };

    Ok(text)
}

#[tokio::main]
pub async fn main() {
    let config_file= OsStr::from_bytes(defaults::DEFAULT_CONFIG_FILE_PATH.as_bytes());
    let config = match get_config(Path::new(config_file)) {
        Ok(v) => {v}
        Err(e) => {eprintln!("Error: Failed to read config file at {config_file:?}.\nError: {e}"); return;}
    };

    let db_path = config.local_database();
    let db = init_db(db_path).await.unwrap();

    println!("Hello, world! {config:?}");

    let projects = get_projects(&config).await;
    match projects {
        Ok(a) => { println!(" Got projects {a}") }
        Err(e) => { println!(" failed to get projects {e}") }
    }
}
