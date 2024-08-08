use crate::get_config::Config;
use crate::get_json_from_url::get_json_from_url;
use crate::get_str_for_key;
use crate::utils::{get_inputs_in_db_not_in_remote, get_inputs_in_remote_not_in_db};
use sqlx::{FromRow, Pool, Sqlite};
use std::collections::HashSet;

#[derive(FromRow, Debug, Eq, PartialEq, Hash)]
pub(crate) struct IssueLinkType {
    jira_id: u32,
    name: String,
    outward_name: String,
    inward_name: String,
}

async fn get_link_types_from_database(db_conn: &Pool<Sqlite>) -> Vec<IssueLinkType> {
    let query_str = "SELECT  jira_id, name, outward_name, inward_name
         FROM IssueLinkType;";

    let rows = sqlx::query_as::<_, IssueLinkType>(query_str)
        .fetch_all(db_conn)
        .await;

    match rows {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Error occurred while trying to get link types from local database: {e}");
            Vec::new()
        }
    }
}

async fn get_issue_link_types_from_server(config: &Config) -> Result<Vec<IssueLinkType>, String> {
    let query = "/rest/api/2/issueLinkType";
    let json_data = get_json_from_url(config, query).await;
    let Ok(json_data) = json_data else {
        return Err(format!(
            "Error: failed to get list of link types from server.\n{e}",
            e = json_data.err().unwrap().to_string()
        ));
    };

    let Some(json_array) = json_data.as_object() else {
        return Err(format!(
            "Error: Returned data is unexpected. Expecting a json object, got [{e}]",
            e = json_data.to_string()
        ));
    };

    let Some(json_array) = json_array.get("issueLinkTypes") else {
        return Err(format!(
            "Error: Returned data is unexpected. Expecting a key named issueLinkTypes, got [{e}]",
            e = json_data.to_string()
        ));
    };

    let Some(json_array) = json_array.as_array() else {
        return Err(format!(
            "Error: Returned data is unexpected. Expecting a json array, got [{e}]",
            e = json_data.to_string()
        ));
    };

    let res = json_array
    .iter()
    .filter_map(|x| {
      let name = get_str_for_key(&x, "name")?;
      let outward_name = get_str_for_key(&x, "outward")?;
      let inward_name = get_str_for_key(&x, "inward")?;
      let jira_id = get_str_for_key(&x, "id")?;
      let jira_id = match jira_id.parse::<u32>() {
        Ok(k) => { k }
        Err(e) => {
          eprintln!("Error: failed to parse a jira_id as integer. id was [{jira_id}]. Error was {e}. Ignoring it");
          return None;
        }
      };

      Some(IssueLinkType {
        jira_id,
        name: name.to_string(),
        outward_name: outward_name.to_string(),
        inward_name: inward_name.to_string()
      })
    })
    .collect::<Vec<_>>();

    //    dbg!(&res);
    Ok(res)
}

fn get_link_types_in_remote_not_in_db<'a, 'b>(
    link_types: &'a Vec<IssueLinkType>,
    link_types_in_db: &'b Vec<IssueLinkType>,
) -> Vec<&'a IssueLinkType>
where
    'b: 'a,
{
    get_inputs_in_remote_not_in_db(link_types.as_slice(), link_types_in_db.as_slice())
}

fn get_link_types_in_db_not_in_remote<'a>(
    link_types_in_remote: &'a Vec<IssueLinkType>,
    link_types_in_db: &'a Vec<IssueLinkType>,
) -> Vec<&'a IssueLinkType> {
    get_inputs_in_db_not_in_remote(link_types_in_remote.as_slice(), link_types_in_db.as_slice())
}

pub(crate) async fn update_issue_link_types_in_db(config: &Config, db_conn: &mut Pool<Sqlite>) {
    let issue_link_types_in_remote = get_issue_link_types_from_server(&config).await;
    let Ok(issue_link_types_in_remote) = issue_link_types_in_remote else {
        eprintln!(
            "Error: failed to get link types from server: Err=[{e}]",
            e = issue_link_types_in_remote.err().unwrap()
        );
        return;
    };
    let issue_link_types_in_db = get_link_types_from_database(&db_conn).await;
    let issue_link_types_to_insert =
        get_link_types_in_remote_not_in_db(&issue_link_types_in_remote, &issue_link_types_in_db);
    let issue_link_types_to_remove =
        get_link_types_in_db_not_in_remote(&issue_link_types_in_remote, &issue_link_types_in_db);

    match issue_link_types_to_remove.is_empty() {
      true => {eprintln!("No issue link type found in local db that isn't also in the remote");}
      false => {
        let query_str = "DELETE FROM IssueLinkType
                      WHERE jira_id = ?;";

        let mut has_error = false;
        let mut row_affected = 0;
        let mut tx = db_conn
          .begin()
          .await
          .expect("Error when starting a sql transaction");

        for IssueLinkType{ jira_id, name, outward_name, inward_name } in issue_link_types_to_remove
        {
          let res = sqlx::query(query_str)
            .bind(jira_id)
            .execute(&mut *tx)
            .await;

          match res {
            Ok(e) => row_affected += e.rows_affected(),
            Err(e) => {
              has_error = true;
              eprintln!("Error when removing an issue link type with jira_id {jira_id}, name: {name}, outward_name: {outward_name}, inward_name: {inward_name}: Err {e}");
            }
          }
        }

        tx.commit().await.unwrap();

        if has_error {
          eprintln!("Error occurred while removing issue link type from the local database")
        } else {
          eprintln!("updated issue link type in database: {row_affected} rows were deleted")
        }
      }
    }

    match issue_link_types_to_insert.is_empty() {
        true => {
            eprintln!("No new issue link type found");
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
    "INSERT INTO IssueLinkType (jira_id, name, outward_name, inward_name) VALUES
                (?, ?, ?, ?)
            ON CONFLICT DO
            UPDATE SET name = excluded.name, inward_name = excluded.inward_name, outward_name = excluded.outward_name";

            for IssueLinkType {
                jira_id,
                name,
                outward_name,
                inward_name,
            } in issue_link_types_to_insert
            {
                let res = sqlx::query(query_str)
                    .bind(jira_id)
                    .bind(name)
                    .bind(outward_name)
                    .bind(inward_name)
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
    }
}
