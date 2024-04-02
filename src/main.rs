#![feature(const_trait_impl)]
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use crate::get_config::get_config;

mod get_config;
mod defaults;

pub fn main() {
    let config_file= OsStr::from_bytes(defaults::DEFAULT_CONFIG_FILE_PATH.as_bytes());
    let config = match get_config(Path::new(config_file)) {
        Ok(v) => {v}
        Err(e) => {eprintln!("Error: failed to read config file at {config_file:?}. Error: {e}"); return;}
    };

    println!("Hello, world! {config:?}");
}
