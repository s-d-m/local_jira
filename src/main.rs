extern crate core;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use base64::Engine;
use sqlx;
use sqlx::{Execute, Executor, FromRow, Pool, Sqlite, SqlitePool, Statement};
use sqlx::migrate::MigrateDatabase;

use crate::get_config::{Config, get_config};
use crate::manage_interesting_projects::update_interesting_projects_in_db;
use crate::manage_field_table::update_fields_in_db;
use crate::manage_issuetype_table::update_issue_types_in_db;
use crate::manage_linktype_table::update_issue_link_types_in_db;
use crate::manage_project_table::update_project_list_in_db;

mod get_config;
mod defaults;
mod manage_interesting_projects;
mod get_project_tasks_from_server;
mod get_json_from_url;
mod manage_linktype_table;
mod manage_field_table;
mod utils;
mod manage_issuetype_table;
mod manage_project_table;
mod manage_issue_field;

async fn init_db(db_path: &std::path::PathBuf) -> Result<Pool<Sqlite>, String> {

    let path = db_path.to_str();
    let Some(path) = path else {
        return Err(format!("Unsupported filename [{f}] must be utf8 valid.",
                           f = db_path.to_string_lossy()));
    };
    if !Sqlite::database_exists(path).await.unwrap_or(false) {
        println!("Creating database {}", path);
        match Sqlite::create_database(path).await {
            Ok(_) => println!("Create db success"),
            Err(error) => panic!("error: {}", error),
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


fn get_str_for_key<'a>(x: &'a serde_json::Value, key_name: &str) -> Option<&'a str> {
    match x.get(key_name) {
        None => {
            eprintln!("Error: returned project does not contained a \"{key_name}\" value in the json. Ignoring it");
            None
        }
        Some(k) => {
            match k.as_str() {
                None => {
                    eprintln!("Error: returned project \"{key_name}\" is not a string in the json. Ignoring it");
                    None
                }
                Some(k) => {
                    Some(k)
                }
            }
        }
    }
}



#[tokio::main]
pub async fn main() {
    let config_file = OsStr::from_bytes(defaults::DEFAULT_CONFIG_FILE_PATH.as_bytes());
    let config = match get_config(Path::new(config_file)) {
        Ok(v) => { v }
        Err(e) => {
            eprintln!("Error: Failed to read config file at {config_file:?}.\nError: {e}");
            return;
        }
    };

    let db_path = config.local_database();
    let mut db = init_db(db_path).await.unwrap();

    update_issue_types_in_db(&config, &mut db).await;
    update_fields_in_db(&config, &mut db).await;
    update_issue_link_types_in_db(&config, &mut db).await;
    update_project_list_in_db(&config, &mut db).await;
    update_interesting_projects_in_db(&config, &mut db).await;
}
