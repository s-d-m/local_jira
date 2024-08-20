use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_field_table::Field;
use crate::manage_interesting_projects::Issue;
use sqlx::sqlite::SqliteQueryResult;
use sqlx::types::JsonValue;
use sqlx::{Error, FromRow, Pool, Sqlite};
use std::collections::{HashMap, HashSet};
use crate::utils::remove_surrounding_quotes;

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
      let Some(created) = created.as_str() else {
        eprintln!("created value has the wrong type. Should be a json string. is '{x}' instead", x = created.to_string());
        return None;
      };


      let Some(modified) = x.get("updated") else {
        eprintln!("expected comment has the wrong format. Missing 'updated' field");
        return None;
      };
      let Some(modified) = modified.as_str() else {
        eprintln!("updated value has the wrong type. Should be a json string. is '{x}' instead", x = modified.to_string());
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
      let created = created.to_string();
      let modified = modified.to_string();
      let created = remove_surrounding_quotes(created);
      let modified = remove_surrounding_quotes(modified);
      Some(commentFromJson {
        author,
        created,
        modified,
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

#[derive(Debug, FromRow, Hash, Eq, PartialEq)]
struct CommentsFromDbForIssue {
  id: i64,
  position_in_array: u32,
  content_data: String,
  author: String,
  creation_time: String,
  last_modification_time: String
}

async fn get_comments_from_db_for_issue(
    issue_id: u32,
    db_conn: &mut Pool<Sqlite>,
) -> Vec<CommentsFromDbForIssue> {
    let query_str =
      "SELECT id, position_in_array, content_data, author, creation_time, last_modification_time
       FROM Comment
       WHERE issue_id = ?
       ORDER BY position_in_array";

    let rows = sqlx::query_as::<_, CommentsFromDbForIssue>(query_str)
        .bind(issue_id)
        .fetch_all(&*db_conn)
        .await;

    match rows {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error occurred while trying to get comments id from local database for issue {issue_id}: {e}");
            Vec::new()
        }
    }
}

#[derive(FromRow)]
struct AccountId {
    account_id: String,
}

fn get_authors_in_comments_not_in_db<'a>(
    authors_in_comments: &[&'a Author],
    authors_in_db: &[AccountId],
) -> Vec<&'a Author> {
    let authors_in_db = authors_in_db
        .iter()
        .map(|x| x.account_id.as_str())
        .collect::<HashSet<_>>();

    let res = authors_in_comments
        .into_iter()
        .map(|x| *x)
        .filter(|x| !authors_in_db.contains(x.accountId.as_str()))
        .collect::<Vec<_>>();

    res
}

struct CommentsDifference<'a> {
  comments_in_db_not_in_remote: Vec<&'a CommentsFromDbForIssue>,
  comments_in_remote_not_in_db: Vec<&'a CommentsFromDbForIssue>
}
fn get_difference_in_comments<'a>(comments_in_remote: &'a [CommentsFromDbForIssue],
                                  comments_in_db: &'a [CommentsFromDbForIssue]) -> CommentsDifference<'a> {

  let comments_in_remote = comments_in_remote
    .iter()
    .collect::<HashSet<_>>();

  let comments_in_db = comments_in_db
    .iter()
    .collect::<HashSet<_>>();

  let comments_in_remote_not_in_db = comments_in_remote
    .difference(&comments_in_db)
    .map(|x| *x)
    .collect::<Vec<_>>();

  let comments_in_db_not_in_remote = comments_in_db
    .difference(&comments_in_remote)
    .map(|x| *x)
    .collect::<Vec<_>>();

  let res = CommentsDifference {
    comments_in_db_not_in_remote,
    comments_in_remote_not_in_db
  };
  res
}


async fn update_comments_in_db(comments_in_remote_for_issue: Vec<commentFromJson>,
                               comments_in_db_for_issue: &[CommentsFromDbForIssue],
                               issue_id:u32, db_conn: &mut Pool<Sqlite>) {
    let authors_in_comments = comments_in_remote_for_issue
      .iter()
      .map(|x| &x.author)
      .collect::<Vec<_>>();

    let query_str = "SELECT accountId as account_id From People";
    let authors_in_db = sqlx::query_as::<_, AccountId>(query_str)
        .fetch_all(&*db_conn)
        .await;

    let authors_in_db = match authors_in_db {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error occurred while fetching the authors in db: {e:?}");
            Vec::new()
        }
    };

    let authors_to_insert =
        get_authors_in_comments_not_in_db(authors_in_comments.as_slice(), authors_in_db.as_slice());

    match authors_to_insert.is_empty() {
        true => {
            eprintln!("No new comment authors found")
        }
        false => {
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

             let query_str = "INSERT INTO People (accountId, displayName) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET displayName = excluded.displayName";

            // first, insert the authors since the comments references them as a foreign key
            for Author {
                accountId,
                displayName,
            } in authors_to_insert
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

            tx.commit().await.unwrap();
        }
    }

  let comments_in_remote_for_issue = comments_in_remote_for_issue
    .into_iter()
    .enumerate()
    .map(|(pos_in_arrau, comment_from_json)| CommentsFromDbForIssue {
      id: comment_from_json.id,
      position_in_array: pos_in_arrau as u32,
      content_data: comment_from_json.content,
      author: comment_from_json.author.accountId,
      creation_time: comment_from_json.created,
      last_modification_time: comment_from_json.modified,
    })
    .collect::<Vec<_>>();


  let comments_difference = get_difference_in_comments(&comments_in_remote_for_issue,
                                                       comments_in_db_for_issue);

  let comments_to_remove = comments_difference.comments_in_db_not_in_remote;
  let comments_to_insert = comments_difference.comments_in_remote_not_in_db;

  // dbg!(&comments_to_remove);
  // dbg!(&comments_to_insert);

  match comments_to_remove.is_empty() {
    true => { eprintln!("No comments was updated or removed for issue with id {issue_id}")}
    false => {
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

      let query_str = "DELETE FROM Comment WHERE id = ?";
      for comment in comments_to_remove {
        let key = comment.id;
        let res = sqlx::query(query_str)
          .bind(key)
          .execute(&mut *tx).await;
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
        eprintln!("Error occurred while updating comments (removing) for issue with id {issue_id}.")
      } else {
        eprintln!("updated Comments in database (removing) for issue with id {issue_id}: {row_affected} rows were updated")
      }
    }
  }

  match comments_to_insert.is_empty() {
    true => {eprintln!("No comments to insert of update for issue with id {issue_id}")}
    false => {
      let mut has_error = false;
      let mut row_affected = 0;

      // todo(perf): these insert are likely very inefficient since we insert
      // one element at a time instead of doing bulk insert.
      // check https://stackoverflow.com/questions/65789938/rusqlite-insert-multiple-rows
      // and https://www.sqlite.org/c3ref/c_limit_attached.html#sqlitelimitvariablenumber
      // for the SQLITE_LIMIT_VARIABLE_NUMBER maximum number of parameters that can be
      // passed in a query.
      // splitting an iterator in chunks would come in handy here.


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

      let mut tx = db_conn
        .begin()
        .await
        .expect("Error when starting a sql transaction");

      for CommentsFromDbForIssue {
        id,
        position_in_array,
        content_data,
        author,
        creation_time,
        last_modification_time
      }
      in comments_to_insert
      {
        let res = sqlx::query(query_str)
          .bind(id)
          .bind(issue_id)
          .bind(position_in_array)
          .bind(content_data)
          .bind(author)
          .bind(creation_time)
          .bind(last_modification_time)
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
  }
}

pub async fn add_comments_for_issue_into_db(
    config: &Config,
    issue_id: u32,
    db_conn: &mut Pool<Sqlite>,
) {
    let comments_in_remote_for_issue = get_comments_from_server_for_issue(&config, issue_id).await;
    let Some(comments_in_remote_for_issue) = comments_in_remote_for_issue else {
      return;
    };

    let comments_in_db_for_issue = get_comments_from_db_for_issue(issue_id, db_conn).await;

    update_comments_in_db(comments_in_remote_for_issue,
                          comments_in_db_for_issue.as_ref(),
                          issue_id, db_conn).await;
}
