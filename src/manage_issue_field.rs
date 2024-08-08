use crate::manage_interesting_projects::{get_id, Issue};
use serde_json::Value;
use sqlx::{FromRow, Pool, Sqlite};
use std::cmp::Ordering;
use std::collections::HashSet;

pub(crate) struct IssueProperties {
    pub(crate) issue_id: u32,
    pub(crate) properties: Vec<(String /* key */, String /* value */)>,
}

fn get_issues_properties(json_data: &Value) -> Result<Vec<IssueProperties>, String> {
    let Some(v) = json_data.get("issues") else {
        return Err(String::from("No field named 'issues' in the json"));
    };

    let Some(v) = v.as_array() else {
        return Err(String::from(
            "Error: the fields named 'issues' isn't a json array",
        ));
    };

    let properties = v
        .iter()
        .filter_map(|x| {
            let Some(issue_id) = get_id(x) else {
                return None;
            };

            let Some(x) = x.as_object() else {
                return None;
            };

            let Some(fields) = x.get("fields") else {
                return None;
            };

            let Some(fields) = fields.as_object() else {
                return None;
            };

            // todo ensure attachments are added here

            let key_values = fields
                .iter()
                .filter_map(|(key, value)| match value.as_null() {
                    Some(()) => None,
                    None => Some((key.to_string(), value.to_string())),
                })
                .collect::<Vec<_>>();

            Some(IssueProperties {
                issue_id,
                properties: key_values,
            })
        })
        .collect::<Vec<_>>();

    Ok(properties)
}

#[derive(Hash, FromRow, Eq, PartialEq)]
struct BrokenIssueProperties {
    issue_id: u32,
    field_id: String,
    field_value: String,
}

fn get_flattened_properties(
    issue_properties: &[IssueProperties],
) -> Vec<(
    u32, /* issue id */
    HashSet<crate::manage_issue_field::BrokenIssueProperties>,
)> {
    let flattened_properties = issue_properties
        .iter()
        .map(|x| {
            let flattened_properties = x
                .properties
                .iter()
                .map(|(key, value)| BrokenIssueProperties {
                    issue_id: x.issue_id,
                    field_id: key.to_string(),
                    field_value: value.to_string(),
                })
                .collect::<HashSet<_>>();
            (x.issue_id, flattened_properties)
        })
        .collect::<Vec<_>>();

    flattened_properties
}

async fn get_flattened_properties_for_issue_in_db(
    issue_id: u32,
    db_conn: Pool<Sqlite>,
) -> (u32 /* issue id */, HashSet<BrokenIssueProperties>) {
    let properties_in_db_qyery = "SELECT issue_id, field_id, field_value
     FROM IssueField
     WHERE issue_id = ?;";

    let res = sqlx::query_as::<_, BrokenIssueProperties>(properties_in_db_qyery)
        .bind(issue_id)
        .fetch_all(&db_conn)
        .await;

    let res = match res {
        Ok(e) => {
            let properties = e.into_iter().collect::<HashSet<_>>();
            (issue_id, properties)
        }
        Err(e) => {
            eprintln!("Error when fetching fields with issue_id = {issue_id}, {e}");
            (issue_id, HashSet::new())
        }
    };

    res
}

async fn get_flattened_properties_from_db(
    ids: &[u32],
    db_conn: Pool<Sqlite>,
) -> Vec<(u32 /* issue id */, HashSet<BrokenIssueProperties>)> {
    let mut handles = ids
        .iter()
        .map(|issue_id| {
            tokio::spawn(get_flattened_properties_for_issue_in_db(
                *issue_id,
                db_conn.clone(),
            ))
        })
        .collect::<tokio::task::JoinSet<_>>();

    let mut flattened_properties_in_db: Vec<(u32, HashSet<BrokenIssueProperties>)> = vec![];
    while let Some(v) = handles.join_next().await {
        match v {
            Ok(Ok(v)) => flattened_properties_in_db.push(v),
            Ok(Err(e)) | Err(e) => {
                eprintln!("Failed to join spawned task {e:?}")
            }
        };
    }
    flattened_properties_in_db.sort_by(|a, b| match (a.0, b.0) {
        (x, y) if x < y => Ordering::Less,
        (x, y) if x == y => Ordering::Equal,
        (x, y) if x > y => Ordering::Greater,
        _ => panic!(),
    });
    flattened_properties_in_db
}

fn get_properties_in_db_not_in_remote<'a>(
    properties_in_remote: &'a [(u32, HashSet<BrokenIssueProperties>)],
    properties_in_db: &'a [(u32, HashSet<BrokenIssueProperties>)],
) -> Vec<&'a BrokenIssueProperties> {
    let properties = properties_in_remote
        .iter()
        .zip(properties_in_db.iter())
        .map(|(a, b)| {
            assert_eq!(a.0, b.0);
            (&a.1, &b.1)
        });

    let mut res = Vec::<&'a _>::new();
    for (properties_in_remote, properties_in_db) in properties {
        let properties_in_db_not_in_remote = properties_in_db.difference(&properties_in_remote);
        let mut properties_in_db_not_in_remote = properties_in_db_not_in_remote
            .into_iter()
            .collect::<Vec<_>>();
        res.append(&mut properties_in_db_not_in_remote);
    }
    res
}

fn get_properties_in_remote_not_in_db<'a>(
    properties_in_remote: &'a [(u32, HashSet<BrokenIssueProperties>)],
    properties_in_db: &'a [(u32, HashSet<BrokenIssueProperties>)],
) -> Vec<&'a BrokenIssueProperties> {
    let properties = properties_in_remote
        .iter()
        .zip(properties_in_db.iter())
        .map(|(a, b)| {
            assert_eq!(a.0, b.0);
            (&a.1, &b.1)
        });

    let mut res = Vec::<&'a _>::new();
    for (properties_in_remote, properties_in_db) in properties {
        let properties_in_remote_not_in_db = properties_in_remote.difference(&properties_in_db);
        let mut properties_in_remote_not_in_db = properties_in_remote_not_in_db
            .into_iter()
            .collect::<Vec<_>>();
        res.append(&mut properties_in_remote_not_in_db);
    }
    res
}

pub(crate) async fn fill_issues_fields(json_data: &Value, db_conn: &mut Pool<Sqlite>) {
    let properties = get_issues_properties(&json_data);
    let properties = match properties {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return;
        }
    };

    let flattened_properties = get_flattened_properties(&properties);
    let ids = properties.iter().map(|a| a.issue_id).collect::<Vec<_>>();
    let flattened_properties_in_db =
        get_flattened_properties_from_db(ids.as_ref(), db_conn.clone()).await;

    assert_eq!(flattened_properties.len(), flattened_properties_in_db.len());
    assert!(flattened_properties
        .iter()
        .zip(flattened_properties_in_db.iter())
        .all(|(a, b)| a.0 == b.0));

    let properties_to_remove = get_properties_in_db_not_in_remote(
        flattened_properties.as_slice(),
        flattened_properties_in_db.as_slice(),
    );
    let properties_to_insert = get_properties_in_remote_not_in_db(
        flattened_properties.as_slice(),
        flattened_properties_in_db.as_slice(),
    );

    match properties_to_remove.is_empty() {
        true => {
            eprintln!("No field issue in local database but deleted from remote found.")
        }
        false => {
            let query_str = "DELETE FROM IssueField
                      WHERE issue_id = ?
                      AND field_id = ?;";

            let mut has_error = false;
            let mut row_affected = 0;
            let mut tx = db_conn
                .begin()
                .await
                .expect("Error when starting a sql transaction");

            for BrokenIssueProperties {
                issue_id,
                field_id,
                field_value,
            } in properties_to_remove
            {
                let res = sqlx::query(query_str)
                    .bind(issue_id)
                    .bind(field_id)
                    .execute(&mut *tx)
                    .await;

                match res {
                    Ok(e) => row_affected += e.rows_affected(),
                    Err(e) => {
                        has_error = true;
                        eprintln!("Error when removing an issue field with (issue_id {issue_id}, field_id: {field_id}, value: {field_value}): {e}");
                    }
                }
            }

            tx.commit().await.unwrap();

            if has_error {
                eprintln!("Error occurred while removing issue fields from the local database")
            } else {
                eprintln!("updated Issue fields in database: {row_affected} rows were deleted")
            }
        }
    }

    match properties_to_insert.is_empty() {
        true => {
            eprintln!("No new field issue detected on remote")
        }
        false => {
            let query_str = "INSERT INTO IssueField (issue_id, field_id, field_value)
                      VALUES (?, ?, ?)
                      ON CONFLICT DO
                      UPDATE SET field_value = excluded.field_value;";

            let mut has_error = false;
            let mut row_affected = 0;
            let mut tx = db_conn
                .begin()
                .await
                .expect("Error when starting a sql transaction");

            for BrokenIssueProperties {
                issue_id,
                field_id,
                field_value,
            } in properties_to_insert
            {
                let res = sqlx::query(query_str)
                    .bind(issue_id)
                    .bind(field_id)
                    .bind(field_value)
                    .execute(&mut *tx)
                    .await;

                match res {
                    Ok(e) => row_affected += e.rows_affected(),
                    Err(e) => {
                        has_error = true;
                        eprintln!("Error when adding an issue field with (issue_id {issue_id}, key: {field_id}, value: {field_value}): {e}");
                    }
                }
            }

            tx.commit().await.unwrap();

            if has_error {
                eprintln!("Error occurred while updating the database with issue fields")
            } else {
                eprintln!("updated Issue fields in database: {row_affected} rows were inserted")
            }
        }
    }
}
