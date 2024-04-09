use std::collections::HashSet;
use serde_json::Value;
use sqlx::{FromRow, Pool, Sqlite};
use sqlx::types::JsonValue;
use tokio::net::tcp::ReuniteError;
use crate::get_config::Config;
use crate::get_project_tasks_from_server::get_project_tasks_from_server;
use crate::manage_project_table::{get_projects_from_db, ProjectShortData};


fn get_fields_from_json<'a>(json_value: Result<Value, String>) -> Result<Vec<(String, String)>, String> {
    let Ok(json_data) = json_value else {
        return Err(format!("Error: couldn't extract fields from json: {e}", e = json_value.err().unwrap()));
    };

    let Some(v) = json_data.get("names") else {
        return Err(String::from("No field named 'names' in the json"));
    };

    let Some(v) = v.as_object() else {
        return Err(String::from("Error: the fields named 'names' isn't a json object"));
    };

    let res = v.iter()
        .filter_map(|(key, value)| {
            match value.as_str() {
                None => {
                    println!("value of field in object named 'names' isn't a string");
                    None
                }
                Some(s) => { Some((key.clone(), s.to_string())) }
            }
        })
        .collect::<Vec<_>>();

    Ok(res)
}


fn get_fields_not_in_db<'a, 'b>(fields: &'a [(String, String)], fields_in_db: &'b [(String, String)])
                                -> Vec<&'a (String, String)>
  where 'b: 'a
{
    // use hash tables to avoid quadratic algorithm
    // todo(perf) use faster hasher. We don't need the security from SIP
    let to_hash_set = |x: &'a [(String, String)]| {
        x
            .iter()
            .collect::<HashSet<&'a (String, String)>>()
    };

    let fields_in_db = to_hash_set(fields_in_db);
    let fields = to_hash_set(fields);

    let res = fields.difference(&fields_in_db)
        .map(|x| *x)
        .collect::<Vec<_>>();
    res
}

#[derive(FromRow, Debug)]
pub(crate) struct fields_in_db {
    jira_field_name: String,
    human_name: String,
}

pub(crate) async fn get_fields_from_db(db_conn: &Pool<Sqlite>) -> Vec<fields_in_db> {
    let query_str =
        "SELECT  jira_field_name, human_name
         FROM fields_name;";

    let rows = sqlx::query_as::<_, fields_in_db>(query_str)
        .fetch_all(db_conn)
        .await;

    rows.unwrap_or_else(|e| {
        eprintln!("Error occurred while trying to get projects from local database: {e}");
        Vec::new()
    })
}


pub(crate) async fn update_db_with_field_names(fields: &[(String, String)], db_conn: &Pool<Sqlite>) {
    if fields.is_empty() {
        // no need to query the db to find out that there won't be any project to insert there
        return;
    }

    // avoid taking write locks on the db if there is nothing to update
    let fields_in_db = get_fields_from_db(db_conn).await;
    let fields_in_db = fields_in_db
        .into_iter()
        .map(|x| (x.jira_field_name.clone(), x.human_name.clone()))
        .collect::<Vec<_>>();

    let fields_to_insert = get_fields_not_in_db(fields, &fields_in_db);

    dbg!(&fields_to_insert);
    dbg!(&fields_in_db);


    if fields_to_insert.is_empty() {
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

    let query_str =
        "INSERT INTO fields_name (jira_field_name, human_name) VALUES
                (?, ?)
            ON CONFLICT DO
            UPDATE SET human_name = excluded.human_name";

    for (jira_id, human_name) in fields_to_insert {
        let res = sqlx::query(query_str)
            .bind(jira_id)
            .bind(human_name)
            .execute(&mut *tx)
            .await;
        match res {
            Ok(e) => { row_affected += e.rows_affected() }
            Err(e) => {
                has_error = true;
                eprintln!("Error: {e}")
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

pub(crate) async fn update_db_with_interesting_projects(config: &Config, db_conn: &mut Pool<Sqlite>) {
    for project_key in config.interesting_projects() {
        let json_tickets = get_project_tasks_from_server(project_key, &config).await;
        let fields = get_fields_from_json(json_tickets);
        if let Ok(fields) = fields {
            update_db_with_field_names(&fields, db_conn).await;
        }

    }
}



