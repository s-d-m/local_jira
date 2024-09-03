extern crate core;

use std::ffi::OsStr;

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use base64::Engine;
use sqlx;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Execute, Executor, FromRow, Pool, Sqlite, SqlitePool, Statement};

use crate::get_config::{get_config, Config};
use crate::get_issue_details::add_details_to_issue_in_db;
use crate::manage_field_table::update_fields_in_db;
use crate::manage_interesting_projects::initialise_interesting_projects_in_db;
use crate::manage_issuelinktype_table::update_issue_link_types_in_db;
use crate::manage_issuetype_table::update_issue_types_in_db;
use crate::manage_project_table::update_project_list_in_db;

// some useful links: https://developer.atlassian.com/cloud/jira/platform/rest/v3/api-group-issues/#api-group-issues
// https://docs.atlassian.com/software/jira/docs/api/REST/9.14.0/#api/2/project-getAllProjects

mod atlassian_document_format;
mod defaults;
mod find_issues_that_need_updating;
mod get_attachment_content;
mod get_config;
mod get_issue_details;
mod get_json_from_url;
mod get_project_tasks_from_server;
mod manage_field_table;
mod manage_interesting_projects;
mod manage_issue_comments;
mod manage_issue_field;
mod manage_issuelinktype_table;
mod manage_issuetype_table;
mod manage_project_table;
mod server;
mod utils;
mod srv_fetch_ticket;
mod srv_fetch_ticket_list;
mod srv_fetch_ticket_key_value_list;
mod srv_fetch_attachment_list_for_ticket;
mod srv_fetch_attachment_content;
mod srv_synchronise_ticket;
mod srv_synchronise_updated;

async fn init_db(db_path: &std::path::PathBuf) -> Result<Pool<Sqlite>, String> {
    let path = db_path.to_str();
    let Some(path) = path else {
        return Err(format!(
            "Unsupported filename [{f}] must be utf8 valid.",
            f = db_path.to_string_lossy()
        ));
    };
    if !Sqlite::database_exists(path).await.unwrap_or(false) {
        eprintln!("Creating database {}", path);
        match Sqlite::create_database(path).await {
            Ok(_) => eprintln!("Create db success"),
            Err(error) => panic!("error: {}", error),
        }
    } else {
        eprintln!("Database already exists");
    }

    let db = SqlitePool::connect(path).await.unwrap();
    let create_schema = include_str!("create_schema.sql");
    let result = sqlx::query(create_schema)
      .execute(&db)
      .await
      .unwrap();
    eprintln!("Create user table result: {:?}", result);
    Ok(db)
}

fn get_str_for_key<'a>(x: &'a serde_json::Value, key_name: &str) -> Option<&'a str> {
    match x.get(key_name) {
        None => {
            eprintln!("Error: returned project does not contained a \"{key_name}\" value in the json. Ignoring it");
            None
        }
        Some(k) => match k.as_str() {
            None => {
                eprintln!("Error: returned project \"{key_name}\" is not a string in the json. Ignoring it");
                None
            }
            Some(k) => Some(k),
        },
    }
}

#[tokio::main]
pub async fn main() {
    let config_file = OsStr::from_bytes(defaults::DEFAULT_CONFIG_FILE_PATH.as_bytes());
    let config = match get_config(Path::new(config_file)) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: Failed to read config file at {config_file:?}.\nError: {e}");
            return;
        }
    };

    let db_path = config.local_database();
    let db = init_db(db_path)
      .await;

    let mut db = match db {
        Ok(v) => {v}
        Err(e) => {
            eprintln!("Error while initialising the database. Err: {e}");
            return;
        }
    };


    {
        let mut db_issue_type_handle = &mut db.clone();
        let mut db_fields_handle = &mut db.clone();
        let mut db_link_types_handles = &mut db.clone();
        let mut db_project_list_handle = &mut db.clone();

        tokio::join!(
            update_issue_types_in_db(&config, &mut db_issue_type_handle),
            update_fields_in_db(&config, &mut db_fields_handle),
            update_issue_link_types_in_db(&config, &mut db_link_types_handles),
            update_project_list_in_db(&config, &mut db_project_list_handle)
        );
    }
//
//     initialise_interesting_projects_in_db(&config, &mut db).await;
// eprintln!("START UPDATING INTERESTING PROJECT");
// //    update_interesting_projects_in_db(&config, &mut db).await;
// eprintln!("STOP UPDATING INTERESTING PROJECT");
    server::server_request_loop(&db).await;
}
