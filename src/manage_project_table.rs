use std::collections::HashSet;
use base64::Engine;
use sqlx::{FromRow, Pool, Sqlite};
use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct ProjectShortData {
    key: String,
    jira_id: u32,
    name: String,
    lead_name: Option<String>,
    lead_id: Option<String>,
}

pub(crate) async fn get_projects_from_server(conf: &Config) -> Result<Vec<ProjectShortData>, String> {
    let query = "/rest/api/2/project?expand=lead";
    let json_data = get_json_from_url(conf, query).await;
    let Ok(json_data) = json_data else {
      return Err(format!("Error: failed to get list of projects from server.\n{e}", e=json_data.err().unwrap().to_string()));
    };

    let Some(json_array) = json_data.as_array() else {
        return Err(format!("Error: Returned data is unexpected. Expecting a json array, got [{e}]", e = json_data.to_string()));
    };

    let res = json_array
        .iter()
        .filter_map(|x| {
            let key = get_str_for_key(&x, "key")?;
            let jira_id = get_str_for_key(&x, "id")?;
            let jira_id = match jira_id.parse::<u32>() {
                Ok(k) => { k }
                Err(e) => {
                    eprintln!("Error: failed to parse a jira_id as integer. id was [{jira_id}]. Error was {e}. Ignoring it");
                    return None;
                }
            };
            let name = get_str_for_key(&x, "name")?;

            let lead = x.get("lead");
            let lead_name = match lead {
                None => { None }
                Some(val) => {
                    match get_str_for_key(val, "displayName") {
                        None => { None }
                        Some(e) => { Some(e.to_string()) }
                    }
                }
            };

            let lead_id = match lead {
                None => { None }
                Some(val) => {
                    match get_str_for_key(val, "accountId") {
                        None => { None }
                        Some(e) => { Some(e.to_string()) }
                    }
                }
            };


            Some(ProjectShortData {
                key: key.to_string(),
                jira_id,
                name: name.to_string(),
                lead_name,
                lead_id,
            })
        })
        .collect::<Vec<_>>();

//    dbg!(&res);
    Ok(res)
}

#[derive(FromRow)]
struct ProjectShortDataSQL {
    key: String,
    jira_id: u32,
    name: String,
    displayName: Option<String>,
    accountId: Option<String>,
}

pub(crate) async fn get_projects_from_db(db_conn: &Pool<Sqlite>) -> Vec<ProjectShortData> {
    let query_str =
        "SELECT  projects.key, projects.jira_id, projects.name, people.accountId, people.displayName
         FROM projects
         JOIN people on people.accountId = projects.lead_id;";

    let rows = sqlx::query_as::<_, ProjectShortDataSQL>(query_str)
        .fetch_all(db_conn)
        .await;

    match rows {
        // todo(perf) simply rename fields in ProjectShortData to avoid the need of this conversion
        Ok(data) => {
            data.into_iter().map(|x| {
                ProjectShortData {
                    key: x.key,
                    jira_id: x.jira_id,
                    name: x.name,
                    lead_name: x.displayName,
                    lead_id: x.accountId,
                }
            }).collect()
        }
        Err(e) => {
            eprintln!("Error occurred while trying to get projects from local database: {e}");
            Vec::new()
        }
    }
}

fn get_projects_not_in_db<'a, 'b>(projects: &'a Vec<ProjectShortData>, projects_in_db: &'b Vec<ProjectShortData>)
                                  -> Vec<&'a ProjectShortData>
    where 'b: 'a
{
    // use hash tables to avoid quadratic algorithm
    // todo(perf) use faster hasher. We don't need the security from SIP
    let to_hash_set = |x: &'a Vec<ProjectShortData>| {
        x
            .iter()
            .collect::<HashSet<&'a ProjectShortData>>()
    };
    let projects_in_db = to_hash_set(projects_in_db);
    let projects = to_hash_set(projects);

    let res = projects.difference(&projects_in_db)
        .map(|x| *x)
        .collect::<Vec<_>>();
    res
}

pub(crate) async fn update_db_with_projects(projects: &Vec<ProjectShortData>, db_conn: &Pool<Sqlite>) {
    if projects.is_empty() {
        // no need to query the db to find out that there won't be any project to insert there
        return;
    }

    // avoid taking write locks on the db if there is nothing to update
    let projects_in_db = get_projects_from_db(db_conn).await;
    let projects_to_insert = get_projects_not_in_db(projects, &projects_in_db);

    dbg!(&projects_to_insert);
    dbg!(&projects_in_db);

    let projects = projects_to_insert;

    let people = projects
        .iter()
        .filter_map(|x| match (&x.lead_id, &x.lead_name) {
            (Some(id), Some(name)) => Some((id, name)),
            _ => None
        })
        .collect::<Vec<_>>();

    let projects = projects
        .iter()
        .map(|x| {
            (x.jira_id, &x.key, &x.name, &x.lead_id)
        })
        .collect::<Vec<_>>();

    if people.is_empty() && projects.is_empty() {
        return;
    }

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

    if !people.is_empty() {

        let query_str =
            "INSERT INTO people (accountId, displayName) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET displayName = excluded.displayName";


        for (id, name) in people {
            let res = sqlx::query(query_str)
                .bind(id)
                .bind(name)
                .execute(&mut *tx)
                .await;
            match res {
                Ok(e) => { row_affected += e.rows_affected() }
                Err(e) => { has_error = true ; eprintln!("Error: {e}") }
            }

        }
    }

    if !projects.is_empty() {
        let query_str =
            "INSERT INTO projects (jira_id, key, name, lead_id) VALUES
                (?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET key = excluded.key, name=excluded.name, lead_id=excluded.lead_id";

        for (jira_id, key, name, lead_id) in projects {
            let res = sqlx::query(query_str)
                .bind(jira_id)
                .bind(key)
                .bind(name)
                .bind(lead_id)
                .execute(&mut *tx)
                .await;
            match res {
                Ok(e) => {row_affected += e.rows_affected()}
                Err(e) => { has_error=true  ; eprintln!("Error: {e}") }
            }
        }

    }

    tx.commit().await.unwrap();

    if has_error {
        println!("Error occurred while updating the database with projects")
    } else {
        println!("updated projects in database: {row_affected} rows were updated")
    }
}
