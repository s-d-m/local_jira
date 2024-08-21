use sqlx::{Error, FromRow};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use reqwest::header::{HeaderValue, ToStrError};
use reqwest::Response;
use tempfile;
use toml::to_string;
use crate::get_config::Config;

#[derive(FromRow)]
struct cookie_expiration {
    value: Option<String>,
    expiry: Option<i64>,
}

fn is_uuid(candidate: &str) -> bool {
    if candidate.len() != 36 {
        return false;
    }

    let dummy = candidate
      .split('-')
      .collect::<Vec<_>>();

    if dummy.len() != 5 {
        return false;
    }
    if dummy[0].len() != 8 {
        return false;
    }
    if dummy[1].len() != 4 {
        return false;
    }
    if dummy[2].len() != 4 {
        return false;
    }
    if dummy[3].len() != 4 {
        return false;
    }
    if dummy[4].len() != 12 {
        return false;
    }

    let is_str_hexa = |s: &str| -> bool { s.chars().all(|c| c.is_ascii_hexdigit())};
    let res = dummy
      .into_iter()
      .all(is_str_hexa);
    dbg!(res);
    res
}

async fn get_jira_tenant_session_cookie(
    moz_cookie_db: &Option<PathBuf>,
) -> Option<cookie_expiration> {
    let Some(moz_cookie_db) = moz_cookie_db else {
        eprintln!("Couldn't retrieve the firefox cookie since no path were given");
        return None;
    };

    // firefox opens the db with exclusive lock. Hence need to copy the file first
    // before reading it.
    let tmpfile = tempfile::NamedTempFile::new();
    let Ok(tmpfile) = tmpfile else {
        eprintln!("Couldn't retrieve the firefox cookie due to inability to create a new cookie file for it");
        return None;
    };

    let copy_ret_code = std::fs::copy(moz_cookie_db, tmpfile.path());
    let Ok(copy_ret_code) = copy_ret_code else {
        eprintln!("Couldn't retrieve the firefox cookie due to inability to copy the cookie file");
        return None;
    };

    let sql_request = "SELECT value, expiry
                            FROM moz_cookies
                            WHERE name = 'tenant.session.token';";
    let tmp_path = tmpfile.path().as_os_str().to_str().unwrap();
    let conn = sqlx::SqlitePool::connect(tmp_path).await.unwrap();
    let res = sqlx::query_as::<_, cookie_expiration>(sql_request)
        .fetch_optional(&conn)
        .await;
    conn.close().await;

    match res {
        Ok(Some(val)) => Some(val),
        _ => None,
    }
}

fn is_cookie_valid(cookie: &cookie_expiration) -> bool {
    if cookie.value.is_none() {
        return false;
    };

    let Some(expiry) = cookie.expiry else {
        return true;
    };

    let since_the_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    return since_the_epoch + 5 < expiry as u64;
}

pub struct file_data {
    pub uuid: Option<String>,
    pub bytes: Option<Vec<u8>>,
}

async fn download_url(attachment_id: i64, config: &Config, cookie: &str) -> file_data {
    let server = config.server_address();
    let url = format!("{server}/rest/api/3/attachment/content/{attachment_id}");

    let client = reqwest::Client::new();
    let response = client.get(url.as_str())
      .header("Cookie", format!("tenant.session.token={cookie}"))
      .send()
      .await;

    let response = match response {
        Ok(v) => {v}
        Err(e) => {
            eprintln!("Error while downloading content for attachment with id {attachment_id}: {}", e.to_string());
            return file_data{ uuid: None, bytes: None };
        }
    };

    if !response.status().is_success() {
        eprintln!("Error while downloading content for attachment with id {attachment_id}. Returned status code is {x}",
        x=response.status().to_string());
        return file_data{ uuid: None, bytes: None };
    }

    let path = response.url().path();
    let splitted_path = path
      .split('/')
      .collect::<Vec<_>>();
    let uuid = if splitted_path.len() >= 3 {
        if splitted_path[0].is_empty() && splitted_path[1] == "file" && is_uuid(splitted_path[2]) {
            Some(splitted_path[2].to_string())
        } else {
            None
        }
    } else {
        None
    };

    let bytes = match response.bytes().await {
        Ok(v) => {Some(v.to_vec())}
        Err(e) => {
            eprintln!("Failed to download the attachment with ID {attachment_id}. Err {e}");
            None
        }
    };

    let res = file_data{
        uuid,
        bytes,
    };
    res
}

pub async fn get_bytes_content(config: &Config, attachment_id: i64) -> file_data {
    eprintln!("Request to download attachment with id {attachment_id}");

    let moz_cookie_db = config.get_mozilla_cookies_db();
    let cookie = get_jira_tenant_session_cookie(moz_cookie_db).await;
    let cookie = match cookie {
        None => {
            eprintln!("Couldn't retrieve the tenant session token cookie.");
            return file_data{
                uuid: None,
                bytes: None,
            };
        }
        Some(v) if is_cookie_valid(&v) => {
          v.value.unwrap()
        },
      _ => {
        eprintln!("tenant session token cookie found but is invalid");
          return file_data{
              uuid: None,
              bytes: None,
          };
      }
    };

    download_url(attachment_id, config, cookie.as_str()).await
}
