use std::env::VarError;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use serde::Deserialize;
use toml::de::Error;
use crate::defaults;

#[derive(Deserialize)]
struct FileOnDiskConfig {
    server_address: String,
    user_login: String, // likely email address
    api_token: Option<String>, // taken from environment variable when not passed.
    local_database: Option<std::path::PathBuf>,
}

impl FileOnDiskConfig {
    pub fn server_address(&self) -> &str {
        &self.server_address
    }
    pub fn user_login(&self) -> &str {
        &self.user_login
    }
    pub fn api_token(&self) -> &Option<String> {
        &self.api_token
    }
    pub fn local_database(&self) -> &Option<std::path::PathBuf> {
        &self.local_database
    }
}

#[derive(Debug)]
pub(crate) struct Config {
    server_address: String,
    user_login: String, // likely email address
    api_token: String, // taken from environment variable when not passed.
    local_database: std::path::PathBuf,
}

impl Config {
    pub fn server_address(&self) -> &str {
        &self.server_address
    }
    pub fn user_login(&self) -> &str {
        &self.user_login
    }
    pub fn api_token(&self) -> &str {
        &self.api_token
    }
    pub fn local_database(&self) -> &std::path::PathBuf {
        &self.local_database
    }
}

fn api_token_from_env() -> Result<String, String> {
    let env_tok = std::env::var(defaults::JIRA_API_TOKEN_ENV_VAR);
    match env_tok {
        Ok(v) => {Ok(v) }
        Err(a) => {Err(format!("Couldn't get environment variable {x}: {a}", x=defaults::JIRA_API_TOKEN_ENV_VAR)) }
    }
}

pub(crate) fn get_config(filepath: &std::path::Path) -> Result<Config, String> {
    let content = match std::fs::read_to_string(filepath) {
        Ok(v) => {v}
        Err(e) => {return Err(e.to_string())}
    };

    let conf = match toml::from_str::<FileOnDiskConfig>(content.as_str()) {
        Ok(v) => {v}
        Err(e) => {return Err(e.to_string()) }
    };

    let local_database = match conf.local_database {
        None => {
            let mut dst = PathBuf::from(filepath);
            dst.pop();
            dst.push(defaults::DEFAULT_DB_NAME);
            dst
        }
        Some(v) => {v}
    };

    let api_token = match conf.api_token {
        None => { match api_token_from_env() {
            Ok(v) => { v }
            Err(a ) => {return Err(format!("Config file does not contain an api_token and couldn't get it from environment variable.\nError: {a}"))}
          }
        },
        Some(v) => { v }
    };

    let conf = Config {
        server_address: conf.server_address,
        user_login: conf.user_login,
        api_token,
        local_database,
    };

    Ok(conf)
}