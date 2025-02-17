use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::manage_issuetype_table::IssueType;
use crate::utils::{get_inputs_in_db_not_in_remote, get_inputs_in_remote_not_in_db};
use serde_json::Value;
use sqlx::types::Json;
use sqlx::{FromRow, Pool, Sqlite};
use std::collections::HashSet;
use std::num::ParseIntError;

#[derive(FromRow, Debug, Eq, PartialEq, Hash)]
pub(crate) struct Project {
    jira_id: u32,
    key: String,
    name: String,
    description: String,
    is_archived: bool,
}

async fn get_projects_from_database(db_conn: &Pool<Sqlite>) -> Vec<Project> {
    let query_str = "SELECT  jira_id, key, name, description, is_archived
         FROM Project;";

    let rows = sqlx::query_as::<_, Project>(query_str)
        .fetch_all(db_conn)
        .await;

    match rows {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error occurred while trying to get projects from local database: {e:?}");
            Vec::new()
        }
    }
}

async fn get_json_projects_from_server(config: &Config) -> Result<Value, String> {
    let query = "/rest/api/2/project?expand=description";
    let json_data = get_json_from_url(config, query).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get list of projects from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };
    Ok(json_data)
}

async fn get_projects_from_server(json_data: &Value) -> Result<Vec<Project>, String> {
    let Some(json_array) = json_data.as_array() else {
        return Err(format!(
            "Error: Returned data is unexpected. Expecting a json object, got [{e}]",
            e = json_data.to_string()
        ));
    };

    let res = json_array
        .into_iter()
        .filter_map(|x| {
            //      dbg!(x);
            let Some(x) = x.as_object() else {
                return None;
            };
            //      dbg!(x);
            let Some(jira_id) = x.get("id") else {
                return None;
            };
            let Some(jira_id) = jira_id.as_str() else {
                return None;
            };
            let Ok(jira_id) = jira_id.parse::<u32>() else {
                return None;
            };
            //      dbg!(jira_id);
            let Some(key) = x.get("key") else {
                return None;
            };
            let Some(key) = key.as_str() else {
                return None;
            };
            //      dbg!(human_name);
            let Some(name) = x.get("name") else {
                return None;
            };
            let Some(name) = name.as_str() else {
                return None;
            };
            let Some(description) = x.get("description") else {
                return None;
            };
            let Some(description) = description.as_str() else {
                return None;
            };
            let is_archived = match x.get("archived") {
                None => false,
                Some(x) => x.as_bool().unwrap_or(false),
            };
            Some(Project {
                jira_id,
                key: key.to_string(),
                name: name.to_string(),
                description: description.to_string(),
                is_archived,
            })
        })
        .collect::<Vec<_>>();

    Ok(res)
}

fn get_projects_not_in_remote_not_in_db<'a, 'b>(
    projects_in_remote: &'a Vec<Project>,
    projects_in_db: &'b Vec<Project>,
) -> Vec<&'a Project>
where
    'b: 'a,
{
    get_inputs_in_remote_not_in_db(projects_in_remote.as_slice(), projects_in_db.as_slice())
}

fn get_projects_in_db_not_in_remote<'a>(
    projects_in_remote: &'a Vec<Project>,
    projects_in_db: &'a Vec<Project>,
) -> Vec<&'a Project> {
    get_inputs_in_db_not_in_remote(projects_in_remote.as_slice(), projects_in_db.as_slice())
}

fn get_issue_types_per_project_in_remote_not_in_db<'a, 'b>(
    issue_types_per_project_in_remote: &'a Vec<IssueTypePerProject>,
    issue_types_per_project_in_db: &'b Vec<IssueTypePerProject>,
) -> Vec<&'a IssueTypePerProject>
where
    'b: 'a,
{
    get_inputs_in_remote_not_in_db(
        issue_types_per_project_in_remote.as_slice(),
        issue_types_per_project_in_db.as_slice(),
    )
}

fn get_issue_types_per_project_in_db_not_in_remote<'a>(
    issue_types_per_project_in_remote: &'a Vec<IssueTypePerProject>,
    issue_types_per_project_in_db: &'a Vec<IssueTypePerProject>,
) -> Vec<&'a IssueTypePerProject> {
    get_inputs_in_db_not_in_remote(
        issue_types_per_project_in_remote.as_slice(),
        issue_types_per_project_in_db.as_slice(),
    )
}

#[derive(FromRow, Eq, PartialEq, Hash)]
struct IssueTypePerProject {
    project_id: u32,
    issue_type_id: u32,
}

async fn get_issue_types_per_project_in_db(db_conn: &Pool<Sqlite>) -> Vec<IssueTypePerProject> {
    let query_str = "SELECT  issue_type_id, project_id
         FROM IssueTypePerProject;";

    let rows = sqlx::query_as::<_, IssueTypePerProject>(query_str)
        .fetch_all(db_conn)
        .await;

    match rows {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error occurred while trying to get issue type per project from local database: {a}",  a = e.to_string() );
            Vec::new()
        }
    }
}

fn get_issue_types_per_project(json_data: &Value) -> Vec<IssueTypePerProject> {
    let Some(json_array) = json_data.as_array() else {
        eprintln!("Error: Returned data is unexpected for issue types per project. Expecting a json object, got [{e}]", e = json_data.to_string());
        return Vec::new();
    };

    let res = json_array
    .into_iter()
    .filter_map(|x| {
      let Some(x) = x.as_object() else {
        eprintln!("Invalid data found while extracting issue types per project. Data is not a json object. It is {m}", m=x.to_string());
        return None;
      };
      let Some(project_id) = x.get("id") else {
        eprintln!("Invalid data found while extracting issue types per project. json object doesn't have a 'id' key");
        return None;
      };
      let Some(project_id) = project_id.as_str() else {
        eprintln!("Invalid data found while extracting issue types per project. 'id' key isn't of type string. It is {m}", m=project_id.to_string());
        return None;
      };
      let project_id = match project_id.parse::<u32>() {
        Ok(v) => {v}
        Err(e) => {
          eprintln!("Invalid data found while extracting issue types per project. Failed to parse project_id as int. Err is {e:?}");
          return None;
        }
      };
      let Some(issue_types) = x.get("issueTypes") else {
        eprintln!("Invalid data found while extracting issue types per project. json object doesn't have a 'issueTypes' key. Data is {x:?}");
        return None;
      };
      let Some(issue_types) = issue_types.as_array() else {
        eprintln!("Invalid data found while extracting issue types per project. 'issueTypes' key isn't of type array. It is {m}", m=issue_types.to_string());
        return None;
      };
      let issues_for_curr_project = issue_types
        .into_iter()
        .filter_map(|x| {
          let Some(x) = x.as_object() else {
            eprintln!("Invalid data found while extracting issue types for the current project. Data is not a json object. It is {m}", m=x.to_string());
            return None;
          };
          let Some(id) = x.get("id") else {
            eprintln!("Invalid data found while extracting issue types for the current project.json object does not have a 'id' key");
            return None;
          };
          let Some(id) = id.as_str() else {
            eprintln!("Invalid data found while extracting issue types for the current project. 'id' key is not of type string");
            return None;
          };
          let id = match id.parse::<u32>() {
            Ok(v) => {v}
            Err(e) => {
              eprintln!("Invalid data found while extracting issue types for the current project. Failed to parse id as int. Err: {e:?}");
              return None;
            }
          };
          Some(id)
        })
        .map(|issue_type_id| IssueTypePerProject {issue_type_id, project_id})
        .collect::<Vec<_>>();
      Some(issues_for_curr_project)
    })
    .flatten()
    .collect::<Vec<_>>();
    res
}

async fn update_projects(json_data: &Value, db_conn: Pool<Sqlite>) {
    let projects_in_remote = get_projects_from_server(&json_data).await;
    let Ok(projects_in_remote) = projects_in_remote else {
        eprintln!(
            "Error: failed to get projects from server: Err=[{e}]",
            e = projects_in_remote.err().unwrap().as_str()
        );
        return;
    };
    let projects_in_db = get_projects_from_database(&db_conn).await;
    let projects_to_insert =
        get_projects_not_in_remote_not_in_db(&projects_in_remote, &projects_in_db);
    let projects_to_remove = get_projects_in_db_not_in_remote(&projects_in_remote, &projects_in_db);

    match projects_to_remove.is_empty() {
        true => {
            eprintln!("No project found in database that isn't in remote")
        }
        false => {
            let query_str = "DELETE FROM Project
         WHERE jira_id = ?;";

            let mut has_error = false;
            let mut row_affected = 0;
            let mut tx = db_conn
                .begin()
                .await
                .expect("Error when starting a sql transaction");

            for Project {
                jira_id,
                key,
                name,
                description,
                is_archived,
            } in projects_to_remove
            {
                let res = sqlx::query(query_str).bind(jira_id).execute(&mut *tx).await;

                match res {
                    Ok(e) => row_affected += e.rows_affected(),
                    Err(e) => {
                        has_error = true;
                        eprintln!("Error when removing a project with (jira_id {jira_id}, key: {key}, name: {name}): {e}");
                    }
                }
            }

            tx.commit().await.unwrap();

            if has_error {
                eprintln!("Error occurred while removing projects from the local database")
            } else {
                eprintln!("updated projects in database: {row_affected} rows were deleted")
            }
        }
    }

    match projects_to_insert.is_empty() {
        true => {
            eprintln!("No new project found");
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

            let query_str =
                "INSERT INTO Project (jira_id, key, name, description, is_archived) VALUES
                (?, ?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET name = excluded.name,
                       is_archived = excluded.is_archived,
                       description = excluded.description,
                       key = excluded.key";

            for Project {
                jira_id,
                key,
                name,
                description,
                is_archived,
            } in projects_to_insert
            {
                let res = sqlx::query(query_str)
                    .bind(jira_id)
                    .bind(key)
                    .bind(name)
                    .bind(description)
                    .bind(is_archived)
                    .execute(&mut *tx)
                    .await;
                match res {
                    Ok(e) => row_affected += e.rows_affected(),
                    Err(e) => {
                        has_error = true;
                        eprintln!("Error occurred while inserting project with jira_id: {jira_id}, key: {key}, name: {name}: Err: {e}")
                    }
                }
            }

            tx.commit().await.unwrap();

            if has_error {
                eprintln!("Error occurred while updating the database with new projects")
            } else {
                eprintln!("updated projects in database: {row_affected} rows were inserted")
            }
        }
    }
}

async fn update_issue_types_per_project(json_data: &Value, db_conn: Pool<Sqlite>) {
    let issue_types_per_project_in_remote = get_issue_types_per_project(&json_data);
    let issue_types_per_project_in_db = get_issue_types_per_project_in_db(&db_conn).await;
    let issue_types_per_project_to_insert = get_issue_types_per_project_in_remote_not_in_db(
        &issue_types_per_project_in_remote,
        &issue_types_per_project_in_db,
    );
    let issue_types_per_project_to_remove = get_issue_types_per_project_in_db_not_in_remote(
        &issue_types_per_project_in_remote,
        &issue_types_per_project_in_db,
    );

    match issue_types_per_project_to_remove.is_empty() {
      true => {
        eprintln!("No types per project found in local db that wasn't on the remote too");
      }
      false => {
        let query_str = "DELETE FROM IssueTypePerProject
                      WHERE project_id = ?
                      AND issue_type_id = ?;";

        let mut has_error = false;
        let mut row_affected = 0;
        let mut tx = db_conn
          .begin()
          .await
          .expect("Error when starting a sql transaction");

        for IssueTypePerProject{ project_id, issue_type_id } in issue_types_per_project_to_remove
        {
          let res = sqlx::query(query_str)
            .bind(project_id)
            .bind(issue_type_id)
            .execute(&mut *tx)
            .await;

          match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
              has_error = true;
              eprintln!("Error when removing an issue type per project with project_id {project_id}, issue_type_id: {issue_type_id}. Err: {e}");
            }
          }
        }

        tx.commit().await.unwrap();

        if has_error {
          eprintln!("Error occurred while removing issue type per project from the local database")
        } else {
          eprintln!("updated issue type per project in database: {row_affected} rows were deleted")
        }
      }
    }

    match issue_types_per_project_to_insert.is_empty() {
        true => {
            eprintln!("No new issue types per project found");
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

            let query_str = "INSERT INTO IssueTypePerProject (project_id, issue_type_id) VALUES
                (?, ?)";

            for IssueTypePerProject {
                project_id,
                issue_type_id,
            } in issue_types_per_project_to_insert
            {
                let res = sqlx::query(query_str)
                    .bind(project_id)
                    .bind(issue_type_id)
                    .execute(&mut *tx)
                    .await;
                match res {
                    Ok(e) => row_affected += e.rows_affected(),
                    Err(e) => {
                        has_error = true;
                        eprintln!("Error occurred when trying to insert into IssueTypePerProject (project_id: {project_id}, issue_type_id: {issue_type_id}) : {e}")
                    }
                }
            }

            tx.commit().await.unwrap();

            if has_error {
                eprintln!("Error occurred while updating the database with IssueTypePerProject")
            } else {
                eprintln!("updated IssueTypePerProject in database: {row_affected} rows were updated")
            }
        }
    }
}

pub(crate) async fn update_project_list_in_db(config: &Config, mut db_conn: &mut Pool<Sqlite>) {
    let json_data = get_json_projects_from_server(&config).await;
    let Ok(json_data) = json_data else {
        eprintln!(
            "Error: failed to get projects from server: Err=[{e}]",
            e = json_data.err().unwrap().as_str()
        );
        return;
    };

    tokio::join!(
        update_projects(&json_data, db_conn.clone()),
        update_issue_types_per_project(&json_data, db_conn.clone())
    );
}
