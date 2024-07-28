use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_field_table::Field;
use sqlx::sqlite::SqliteQueryResult;
use sqlx::types::JsonValue;
use sqlx::{Error, FromRow, Pool, Sqlite};
use std::collections::{HashMap, HashSet};
use crate::manage_interesting_projects::Issue;

#[derive(Debug)]
struct Author {
    accountId: String,
    displayName: String,
}

#[derive(Debug)]
struct commentFromJson {
    author: Author,
    created: String,
    modified: String,
    content: String,
    issue_id: u32,
    id: i64,
}

async fn get_comments_as_json_for_issue(
    config: &Config,
    issue_key: u32,
) -> Result<JsonValue, String> {
    let query = format!("/rest/api/3/issue/{issue_key}/comment");
    let json_data = get_json_from_url(config, query.as_str()).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get comments for issue {issue_key} from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };
    Ok(json_data)
}

async fn get_comments_from_server_for_issue(
    config: &Config,
    issue_id: u32,
) -> Option<Vec<commentFromJson>> {
    let comments = get_comments_as_json_for_issue(config, issue_id).await;
    let comments = match comments {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return None;
        }
    };

    let comments = match comments.as_object() {
        None => {
            eprintln!("Received json for comments of {issue_id} not as expected. Expecting a json object got {x}",
                x = comments.to_string());
            return None;
        }
        Some(x) => x,
    };

    let comments = match comments.get("comments") {
        None => {
            eprintln!("Received json for comments of {issue_id} does not contain a comments key. json is {comments:?}");
            return None;
        }
        Some(x) => x,
    };

    let comments = match comments.as_array() {
        None => {
            eprintln!("Received json for comments of {issue_id} does not contain a comments key. json is {x}",
                x = comments.to_string());
            return None;
        }
        Some(x) => x,
    };

    let comments = comments
    .into_iter()
    .filter_map(|x| {
      let Some(x) = x.as_object() else {
        eprintln!("expected comment has the wrong format. Expected json object. Got {a}", a=x.to_string());
        return None;
      };

      let Some(created) = x.get("created") else {
        eprintln!("expected comment has the wrong format. Missing 'created' field");
        return None;
      };

      let Some(modified) = x.get("updated") else {
        eprintln!("expected comment has the wrong format. Missing 'updated' field");
        return None;
      };
      let Some(content) = x.get("body") else {
        eprintln!("expected comment has the wrong format. Missing 'updated' field");
        return None;
      };

      let Some(author) = x.get("author") else {
        eprintln!("expected comment has the wrong format. Missing 'author' field");
        return None;
      };
      let Some(author) = author.as_object() else {
        eprintln!("expected comment has the wrong format. 'author' should be a json object, but instead is {author}");
        return None;
      };
      let Some(author_account_id) = author.get("accountId") else {
        eprintln!("expected comment has the wrong format. 'author' should contain an accountId. Instead it is {author:?}");
        return None;
      };
      let Some(author_account_id) = author_account_id.as_str() else {
        eprintln!("Invalid comment format. 'author account id' should be a json string. Instead, it is {author_account_id}");
        return None;
      };
      let Some(author_display_name) = author.get("displayName") else {
        eprintln!("expected comment has the wrong format. 'author' should contain a displayName. Instead it is {author:?}");
        return None;
      };
      let Some(author_display_name) = author_display_name.as_str() else {
        eprintln!("Invalid comment format. 'author display name' should be a json string. Instead, it is {author_display_name}");
        return None;
      };

      let author = Author {
        accountId: author_account_id.to_string(),
        displayName: author_display_name.to_string()
      };

      let Some(id) = x.get("id") else {
        eprintln!("expected comment has the wrong format. Missing 'id' field");
        return None;
      };

      let Some(id) = id.as_str() else {
        eprintln!("expected comment has the wrong format. 'id' field is not a json string. It is {id}");
        return None;
      };
      let id = match str::parse::<i64>(id) {
        Ok(x) => {x}
        Err(e) => {
          eprintln!("expected comment has the wrong format. Can't get a i64 out of 'id'. id is {id}, err is {e}");
          return None;
        }
      };
      Some(commentFromJson {
        author,
        created: created.to_string(),
        modified: modified.to_string(),
        content: content.to_string(),
        issue_id,
        id,
      })
    }).collect::<Vec<_>>();

    Some(comments)
}

#[derive(FromRow)]
struct IssueId {
    id: i64,
}

async fn get_comments_id_from_db_for_issue(
    issue_id: u32,
    db_conn: &mut Pool<Sqlite>,
) -> Option<Vec<IssueId>> {
    let query_str = "SELECT id
     FROM Comment
     WHERE issue_id = ?";

    let rows = sqlx::query_as::<_, IssueId>(query_str)
        .fetch_all(&*db_conn)
        .await;

    match rows {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("Error occurred while trying to get comments id from local database for issue {issue_id}: {e}");
            None
        }
    }
}

fn get_comments_in_db_not_in_remote<'a>(
    comments_from_remote: &[commentFromJson],
    comments_in_local_db: &'a [IssueId],
) -> Vec<&'a IssueId> {
    let ids_on_remote = comments_from_remote
        .iter()
        .map(|x| x.id)
        .collect::<HashSet<_>>();

    let ids_in_db_not_in_remote = comments_in_local_db
        .iter()
        .filter(|x| !ids_on_remote.contains(&x.id))
        .collect::<Vec<_>>();

    ids_in_db_not_in_remote
}

async fn remove_comments_no_longer_on_remote(
    comments_ids_to_remove: &[IssueId],
    db_conn: &mut Pool<Sqlite>,
) {
    if comments_ids_to_remove.is_empty() {
        return;
    }

    let mut has_error = false;
    let mut row_affected = 0;

    let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

    // todo(perf): these delete are likely very inefficient since we delete
    // one element at a time instead of doing bulk delete.
    // check https://stackoverflow.com/questions/65789938/rusqlite-insert-multiple-rows
    // and https://www.sqlite.org/c3ref/c_limit_attached.html#sqlitelimitvariablenumber
    // for the SQLITE_LIMIT_VARIABLE_NUMBER maximum number of parameters that can be
    // passed in a query.
    // splitting an iterator in chunks would come in handy here.

    let query_str = "DELETE FROM Comments WHERE id = ?";
    for key in comments_ids_to_remove {
        let res = sqlx::query(query_str)
          .bind(key.id)
          .execute(&mut *tx)
          .await;
        match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
                has_error = true;
                eprintln!("Error: {e}")
            }
        }
    }

    tx.commit().await.unwrap();

    if has_error {
        eprintln!("Error occurred while updating the database with Link types")
    } else {
        eprintln!("updated Link types in database: {row_affected} rows were updated")
    }
}

async fn add_comments_in_db(comments: &[commentFromJson], db_conn: &mut Pool<Sqlite>) {
    let authors = comments
      .iter()
      .map(|x| &x.author)
      .collect::<Vec<_>>();

    let mut has_error = false;
    let mut row_affected = 0;

    let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

    // todo(perf): these insert are likely very inefficient since we insert
    // one element at a time instead of doing bulk insert.
    // check https://stackoverflow.com/questions/65789938/rusqlite-insert-multiple-rows
    // and https://www.sqlite.org/c3ref/c_limit_attached.html#sqlitelimitvariablenumber
    // for the SQLITE_LIMIT_VARIABLE_NUMBER maximum number of parameters that can be
    // passed in a query.
    // splitting an iterator in chunks would come in handy here.

    // todo(perf): add detection of what is already in db and do some filter out. Here we happily
    // overwrite data with the exact same ones, thus taking the write lock on the
    // database for longer than necessary.
    // Plus it means the logs aren't that useful to troubleshoot how much data changed
    // in the database. Seeing messages saying
    // 'updated Issue fields in database: 58 rows were updated'
    // means there has been at most 58 changes. Chances are there are actually been
    // none since the last update.
    let query_str = "INSERT INTO People (accountId, displayName) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET displayName = excluded.displayName";

    // first, insert the authors since the comments references as a foreign key
    for Author {
        accountId,
        displayName,
    } in authors
    {
        let res = sqlx::query(query_str)
            .bind(accountId)
            .bind(displayName)
            .execute(&mut *tx)
            .await;
        match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
                has_error = true;
                eprintln!("Error: {e}")
            }
        }
    }

  if has_error {
    eprintln!("Error occurred while updating the database with Authors")
  } else {
    eprintln!("updated Authors in database: {row_affected} rows were updated")
  }

  let mut has_error = false;
  let mut row_affected = 0;
  
    // todo(perf): these insert are likely very inefficient since we insert
    // one element at a time instead of doing bulk insert.
    // check https://stackoverflow.com/questions/65789938/rusqlite-insert-multiple-rows
    // and https://www.sqlite.org/c3ref/c_limit_attached.html#sqlitelimitvariablenumber
    // for the SQLITE_LIMIT_VARIABLE_NUMBER maximum number of parameters that can be
    // passed in a query.
    // splitting an iterator in chunks would come in handy here.

    // todo(perf): add detection of what is already in db and do some filter out. Here we happily
    // overwrite data with the exact same ones, thus taking the write lock on the
    // database for longer than necessary.
    // Plus it means the logs aren't that useful to troubleshoot how much data changed
    // in the database. Seeing messages saying
    // 'updated Issue fields in database: 58 rows were updated'
    // means there has been at most 58 changes. Chances are there are actually been
    // none since the last update.
    let query_str = "INSERT INTO Comment (id, issue_id, position_in_array, content_data, author,
                          creation_time, last_modification_time
                          ) VALUES
                (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET issue_id = excluded.issue_id,
                       position_in_array = excluded.position_in_array,
                       content_data = excluded.content_data,
                       author = excluded.author,
                       creation_time = excluded.creation_time,
                       last_modification_time = excluded.last_modification_time";

    for (
        commentFromJson {
            author,
            created,
            modified,
            content,
            issue_id,
            id,
        },
        pos_in_iterator,
    ) in comments.iter().zip(0..)
    {
        let res = sqlx::query(query_str)
            .bind(id)
            .bind(issue_id)
            .bind(pos_in_iterator)
            .bind(content)
            .bind(&author.accountId)
            .bind(created)
            .bind(modified)
            .execute(&mut *tx)
            .await;
        match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
                has_error = true;
                eprintln!("Error: {e}")
            }
        }
    }

    tx.commit().await.unwrap();

    if has_error {
        eprintln!("Error occurred while updating the database with Comments")
    } else {
        eprintln!("updated Comments in database: {row_affected} rows were updated")
    }
}

pub async fn add_comments_for_issue_into_db(
    config: &Config,
    issue_id: u32,
    db_conn: &mut Pool<Sqlite>,
) {
    let Some(comments_in_remote) = get_comments_from_server_for_issue(&config, issue_id).await
    else {
        return;
    };
    let comment_ids_in_db = get_comments_id_from_db_for_issue(issue_id, db_conn).await;
    let comments_ids_in_db = comment_ids_in_db.unwrap_or_default();

    let comments_to_remove =
        get_comments_in_db_not_in_remote(comments_in_remote.as_ref(), comments_ids_in_db.as_ref());

    let comments_to_remove = comments_to_remove
      .into_iter()
      .map(|x| IssueId{ id: x.id})
      .collect::<Vec<_>>();

    remove_comments_no_longer_on_remote(comments_to_remove.as_ref(), db_conn).await;
    add_comments_in_db(comments_in_remote.as_ref(), db_conn).await;
}
