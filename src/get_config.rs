use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use serde::Deserialize;
use toml::de::Error;
use crate::defaults;

#[derive(Deserialize, Debug)]
pub(crate) struct FileOnDiskConfig {
    server_address: String,
    port: Option<u16>,
    user_login: String, // likely email address
    api_token: Option<String>, // taken from environment variable when not passed.
    local_database: Option<std::path::PathBuf>,
}

impl FileOnDiskConfig {
    pub fn server_address(&self) -> &str {
        &self.server_address
    }
    pub fn port(&self) -> Option<u16> {
        self.port
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


pub(crate) fn get_config(filepath: &std::path::Path) -> Result<FileOnDiskConfig, String> {
    let content = match std::fs::read_to_string(filepath) {
        Ok(v) => {v}
        Err(e) => {return Err(e.to_string())}
    };

    let mut conf = match toml::from_str::<FileOnDiskConfig>(content.as_str()) {
        Ok(v) => {v}
        Err(e) => {return Err(e.to_string()) }
    };

    if conf.local_database.is_none() {
        let mut dst = PathBuf::from(filepath);
        dst.pop();
        dst.push(defaults::DEFAULT_DB_NAME);
        conf.local_database = Some(dst);
    }

    Ok(conf)
}