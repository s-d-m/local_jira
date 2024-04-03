use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use sqlx::migrate::MigrateDatabase;
use sqlx::{Pool, Sqlite, SqlitePool};

use crate::get_config::get_config;

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
}
