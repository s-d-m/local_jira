use std::{io, sync, thread};
use std::fmt::format;
use std::io::{ErrorKind, Read, read_to_string};
use std::ptr::{addr_of_mut, read};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::Duration;

use sqlx::{Pool, Sqlite};
use tokio::task::JoinSet;
use tokio::time::sleep;
use crate::find_issues_that_need_updating::update_interesting_projects_in_db;
use crate::get_config::Config;
use crate::manage_field_table::update_fields_in_db;
use crate::manage_interesting_projects::initialise_interesting_projects_in_db;
use crate::manage_issuelinktype_table::update_issue_link_types_in_db;
use crate::manage_issuetype_table::update_issue_types_in_db;
use crate::manage_project_table::update_project_list_in_db;
use crate::server::RequestKind::Push_error_message;
use crate::srv_fetch_attachment_content::serve_fetch_attachment_content;
use crate::srv_fetch_attachment_list_for_ticket::serve_fetch_ticket_attachment_list;
use crate::srv_fetch_ticket::serve_fetch_ticket_request;
use crate::srv_fetch_ticket_key_value_list::serve_fetch_ticket_key_value_fields;
use crate::srv_fetch_ticket_list::serve_fetch_ticket_list_request;
use crate::srv_synchronise_all::serve_synchronise_all;
use crate::srv_synchronise_ticket::serve_synchronise_ticket;
use crate::srv_synchronise_updated::serve_synchronise_updated_tickets;


#[derive(Eq, PartialEq)]
enum RequestKind {
  Fetch_Ticket(String /* issue key */),
  Fetch_Ticket_List,
  Fetch_Ticket_Key_Value_Fields(String /* issue key */),
  Fetch_Attachment_List_For_Ticket(String /* issue key */),
  Fetch_Attachment_Content(String /* attachment uuid */),
  Synchronise_Ticket(String /* issue key */),
  Synchronise_Updated,
  Synchronise_All,
  Exit_Server_After_Requests,
  Exit_Server_Now,
  Push_error_message(String),
}

struct Request {
  request_id: String,
  request_kind: RequestKind,
}

fn is_valid_request_id(candidate: &str) -> bool {
  if candidate.is_empty() {
    false
  } else {
    let res = candidate
      .chars()
      .all(|x| x.is_ascii_alphanumeric() || (x == '-'));
    res
  }
}

fn is_valid_issue_key(candidate: &str) -> bool {
  // checks that candidate looks like PROJ-123

  let chunks = candidate
    .split('-')
    .collect::<Vec<_>>();
  if chunks.len() != 2 {
    return false;
  }

  // ensures first part is all uppercase
  let is_likely_jira_proj = chunks[0]
    .chars()
    .all(|x| x.is_ascii_uppercase());

  // ensures first part is all digits
  let is_likely_ticket_number = chunks[1]
    .chars()
    .all(|x| x.is_ascii_digit());

  is_likely_jira_proj && is_likely_ticket_number
}

impl Request {
  fn from(line: &str) -> Result<Request, String> {
    let chunks = line
      .split(' ')
      .collect::<Vec<_>>();

    let nr_chunks = chunks.len();
    if (nr_chunks != 3) && (nr_chunks != 2) {
      return Err(String::from("invalid request. Must contain three space separated chunks (last chunk potentially being the empty string)"));
    };

    let candidate_request_id = chunks[0];
    let command = chunks[1];
    let command_parameter = if nr_chunks == 2 { None } else { Some(chunks[2]) };

    if !is_valid_request_id(candidate_request_id) {
      return Err(String::from("Invalid request. Request id should only contain ascii alphanum characters or dashed"));
    }

    let request_id = candidate_request_id.to_string();
    match command {
      "FETCH_TICKET" => {
        match command_parameter {
          None => {
            Err(String::from("Invalid request. Fetch_Ticket takes parameters."))
          },
          Some(command_parameter) => {
            Ok(Request{
              request_id,
              request_kind: RequestKind::Fetch_Ticket(command_parameter.to_string()),
            })
          }
        }
      },
      "FETCH_TICKET_LIST" => {
        match command_parameter {
          None => {
            Ok(Request {
              request_id,
              request_kind: RequestKind::Fetch_Ticket_List,
            })
          },
          Some(command_parameter) => {
            Err(format!("Invalid request. Fetch_Ticket_List doesn't take parameter, but given [{command_parameter}]"))
          }
        }
      },
      "FETCH_TICKET_KEY_VALUE_FIELDS" => {
        match command_parameter {
          None => {
            Err(String::from("Invalid request. Fetch_Ticket_Key_Value_Fields takes a jira issue key as parameter. Something like PROJ-123"))
          },
          Some(command_parameter) => {
            Ok(Request{
              request_id,
              request_kind: RequestKind::Fetch_Ticket_Key_Value_Fields(command_parameter.to_string()),
            })
          }
        }
      },
      "FETCH_ATTACHMENT_LIST_FOR_TICKET" => {
        match command_parameter {
          None => {
            Err(String::from("Invalid request. Fetch_Attachment_List_For_Ticket takes a jira issue key as parameter. Something like PROJ-123"))
          }
          Some(command_parameter) => {
            Ok(Request{
              request_id,
              request_kind: RequestKind::Fetch_Attachment_List_For_Ticket(command_parameter.to_string()),
            })
          }
        }
      }
      "FETCH_ATTACHMENT_CONTENT" => {
        match command_parameter {
          None => {
            Err(String::from("Invalid request. Fetch_Attachment_Content takes a uuid as parameter. Something like PROJ-123"))
          },
          Some(command_parameter) => {
            Ok(Request{
              request_id,
              request_kind: RequestKind::Fetch_Attachment_Content(command_parameter.to_string()),
            })
          }
        }
      }
      "SYNCHRONISE_TICKET" => {
        match command_parameter {
          None => {
            Err(String::from("Invalid request. Synchronise_Ticket takes a jira issue key as parameter. Something like PROJ-123"))
          },
          Some(command_parameter) => {
            Ok(Request{
              request_id,
              request_kind: RequestKind::Synchronise_Ticket(command_parameter.to_string()),
            })
          }
        }
      }
      "SYNCHRONISE_UPDATED" => {
        match command_parameter {
          None => {
            Ok(Request {
              request_id,
              request_kind: RequestKind::Synchronise_Updated,
            })
          },
          Some(command_parameter) => {
            Err(format!("Invalid request. Synchronise_Updated doesn't take parameter. Got [{command_parameter}]"))
          }
        }
      }
      "SYNCHRONISE_ALL" => {
        match command_parameter {
          None => {
            Ok(Request {
              request_id,
              request_kind: RequestKind::Synchronise_All,
            })
          },
          Some(command_parameter) => {
            Err(format!("Invalid request. Synchronise_All doesn't take parameter. Got [{command_parameter}]"))
          }
        }
      }
      "EXIT_SERVER_AFTER_REQUESTS" => {
        match command_parameter {
          None => {
            Ok(Request {
              request_id,
              request_kind: RequestKind::Exit_Server_After_Requests,
            })
          },
          Some(command_parameter) => {
            Err(format!("Invalid request. Exit_Server_After_Requests doesn't take parameter. Got [{command_parameter}]"))
          }
        }
      }
      "EXIT_SERVER_NOW" => {
        match command_parameter {
          None => {
            Ok(Request {
              request_id,
              request_kind: RequestKind::Exit_Server_Now,
            })
          },
          Some(command_parameter) => {
            Err(format!("Invalid request. Exit_Server_Now doesn't take parameter. Got [{command_parameter}]"))
          }
        }
      }
      _ => Err(format!("invalid request, unknown command [{command}]"))
    }
  }
}

pub(crate) struct Reply(pub String);

async fn serve_request(config: Config, request: Request, out_for_replies: tokio::sync::mpsc::Sender<Reply>, mut db_conn: Pool<Sqlite>) {
  let Request { request_id, request_kind: request } = request;
  let request_id = request_id.as_str();
  match request {
    RequestKind::Fetch_Ticket(params) => { serve_fetch_ticket_request(config, request_id, params.as_str(), out_for_replies, &mut db_conn).await }
    RequestKind::Fetch_Ticket_List => {serve_fetch_ticket_list_request(config, request_id, out_for_replies, &mut db_conn).await }
    RequestKind::Fetch_Ticket_Key_Value_Fields(params) => {
      serve_fetch_ticket_key_value_fields(config, request_id, params.as_str(), out_for_replies, &mut db_conn).await
    }
    RequestKind::Fetch_Attachment_List_For_Ticket(params) => {
      serve_fetch_ticket_attachment_list(config, request_id, params.as_str(), out_for_replies, &mut db_conn).await
    }
    RequestKind::Fetch_Attachment_Content(params) => {
      serve_fetch_attachment_content(request_id, params.as_str(), out_for_replies, &mut db_conn).await
    }
    RequestKind::Synchronise_Ticket(params) => {
      serve_synchronise_ticket(config, request_id, params.as_str(), out_for_replies, &mut db_conn).await
    }
    RequestKind::Synchronise_Updated => {
      serve_synchronise_updated_tickets(config, request_id, out_for_replies, &mut db_conn).await
    }
    RequestKind::Synchronise_All => {
      serve_synchronise_all(config, request_id, out_for_replies, &mut db_conn).await
    }
    RequestKind::Exit_Server_After_Requests => { return }
    RequestKind::Exit_Server_Now => { return }
    RequestKind::Push_error_message(s) => {
      let err_msg = if s.is_empty() {
        format!("{request_id} ERROR\n")
      } else {
        format!("{request_id} ERROR {s}\n")
      };
      let _ = out_for_replies.send(Reply(err_msg)).await;
    }
  }
}

async fn process_events(config: Config,
                        mut events_to_process: tokio::sync::mpsc::Receiver<Request>,
                        out_for_replies: tokio::sync::mpsc::Sender<Reply>,
                        db_conn: Pool<Sqlite>) {
  let mut exit_requested = false;
  let mut exit_immediately_requested = false;

  let mut handles = JoinSet::new();
  let mut id_of_exit_request = String::new();
  let mut id_of_exit_immediate_request = String::new();

  while !exit_requested {
    let event = events_to_process.try_recv();
    match event {
      Ok(request) => {
        match request.request_kind {
          RequestKind::Exit_Server_After_Requests => {
            exit_requested = true;
            let _ = out_for_replies.try_send(Reply(format!("{id} ACK\n", id = request.request_id)));
            id_of_exit_request = request.request_id;
          }
          RequestKind::Exit_Server_Now => {
            exit_requested = true;
            exit_immediately_requested = true;
            let _ = out_for_replies.try_send(Reply(format!("{id} ACK\n", id = request.request_id)));
            id_of_exit_immediate_request = request.request_id;
          },
          _ => {
            handles.spawn(serve_request(config.clone(), request, out_for_replies.clone(), db_conn.clone()));
          }
        }
      }
      Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
        tokio::time::sleep(Duration::from_millis(100)).await;
      }
      Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
        exit_requested = true;
      }
    }

    // remove handles of finished task from set
    while let Some(Ok(_)) = handles.try_join_next() {
    }
  }

  while (!exit_immediately_requested) && (!handles.is_empty()) {
    // remove handles of finished task from set
    while let Some(Ok(_)) = handles.try_join_next() {
    }

    let event = events_to_process.try_recv();
    match event {
      Ok(Request { request_id: id, request_kind: RequestKind::Exit_Server_Now }) => {
        exit_immediately_requested = true;
        let _ = out_for_replies.try_send(Reply(format!("{id} ACK\n")));
        id_of_exit_immediate_request = id;
      },
      Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
        if !handles.is_empty() {
          eprintln!("There are still events to be processed apparently");
          tokio::time::sleep(Duration::from_millis(100)).await;
        }
      }
      Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
        exit_immediately_requested = true;
      },
      _ => {}
    }
  }

  drop(events_to_process);


  handles.abort_all();
  if !id_of_exit_request.is_empty() {
    let _ = out_for_replies.try_send(Reply(format!("{id_of_exit_request} FINISHED\n")));
  }
  if !id_of_exit_immediate_request.is_empty() {
    let _ = out_for_replies.try_send(Reply(format!("{id_of_exit_immediate_request} FINISHED\n")));
  }

  drop(out_for_replies);
}

fn is_stdin_closed() -> bool {
  // checking if stdin is closed is based on this answer:
  // https://unix.stackexchange.com/a/626425
  //
  // unfortunately this doesn't work because of earlier calls to openat
  // see https://github.com/rust-lang/libc/issues/3907 for details
  // might as well return false directly.
  // The issue here is that if stdin is closed, the server can't do
  // anything useful and should quit. Instead, it takes 100% CPU when
  // that happens. :-(
  false

  /*
  let mut pollfds_input = libc::pollfd {
    fd: 0, // stdin
    events: 0,
    revents: libc::POLLHUP,
  };
  let pollfds_input_ptr = addr_of_mut!(pollfds_input);

  let poll_res = unsafe {
     libc::poll( pollfds_input_ptr, 1, 0)
  };
  let is_stdin_closed = poll_res > 0;
  is_stdin_closed
  */
}

fn stdin_to_request(request_queue: tokio::sync::mpsc::Sender<Request>) {
  let mut stdin_input: String = Default::default();
  let mut nag_user_about_blocking_stdin = true;

  while (!request_queue.is_closed()) && (!is_stdin_closed()) {
    // When changing code here, make sure that a request to exit the server doesn't require
    // the user to first type enter a second time for the request to be processed. This can
    // easily happen when the blocking call to read from stdin is done on the same thread
    // managing background tasks. Therefore, keep this code in a dedicated thread (not a
    // tokio thread)

    stdin_input.clear();
    let read_line_ret = io::stdin().read_line(&mut stdin_input);
    match read_line_ret {
      Ok(0) => {
        // workaround for not being able to detect closed stdin. In case stdin is
        // closed, ideally the server would gracefully shutdown. Unfortunately
        // detecting if stdin is closed or not doesn't work at the moment.
        // When stdin is closed, read_line returns immediately saying it read 0 bytes.
        // This leads to this loop becoming a busy loop and the program starts taking
        // up 100% CPU.
        // However, read_line can also return Ok(0) with stdin still being open,
        // namely when a user sends an EOF (ctrl+D on the terminal). Therefore, we
        // can't just rely on receiving Ok(0) to detect closed stdin.
        // One way to deal with this would be to change the communication protocol
        // and say receiving an EOF is similar to an exit-after-requests request.
        // There might however be genuine situation where EOF can be received and
        // thus this isn't a feasible situation.
        // Another possibility would be to check the rate of receiving Ok(0). If we
        // are receiving say 1000 consecutive Ok(0) in less than 0.5 seconds, chances
        // are that stdin is closed and not that someone is flooding the server
        // with EOF. However, here we will keep things simple and just sleep a bit to
        // avoid busy looping.
        thread::sleep(Duration::from_millis(50));
      }
      Ok(_) => {
        let without_suffix = stdin_input.strip_suffix('\n');
        let without_suffix = match without_suffix {
          None => { stdin_input.as_str() }
          Some(data) => { data }
        };

        if !without_suffix.is_empty() {
          let request = Request::from(without_suffix);
          let request = match request {
            Ok(v) => { v }
            Err(e) => {
              let request_kind = Push_error_message(format!("Failed to get a request out of [{without_suffix}]: Err: {e}"));
              let request = Request {
                request_id: String::from("_"),
                request_kind
              };
              request
            }
          };
          let _ = request_queue.blocking_send(request);
        }
      }
      Err(e) => {
        if e.kind() == ErrorKind::WouldBlock  {
          if nag_user_about_blocking_stdin {
            nag_user_about_blocking_stdin = false;
            eprintln!("Warning: stdin of server is nonblocking. Server will go into degraded performance mode. Use a blocking stdin for max efficiency");
            // todo call fcntl to change stdin to blocking automatically, and don't nag the user
          }
          // sleep to avoid busy looping
          thread::sleep(Duration::from_millis(20));
        } else {
          eprintln!("Failed to read line from stdin: {e:?}")
        }
      }
    }
  }

  if (is_stdin_closed()) && (!request_queue.is_closed()) {
    let request = Request {
      request_id: "_exit-after-requests-due-to-closed-stdin".to_string(),
      request_kind: RequestKind::Exit_Server_After_Requests
    };
    let _ = request_queue.blocking_send(request);
  }
}

async fn update_jira_schema(config: &Config, db_conn: &Pool<Sqlite>) {
    let mut db_issue_type_handle = &mut db_conn.clone();
    let mut db_fields_handle = &mut db_conn.clone();
    let mut db_link_types_handles = &mut db_conn.clone();
    let mut db_project_list_handle = &mut db_conn.clone();

    tokio::join!(
            update_issue_types_in_db(&config, &mut db_issue_type_handle),
            update_fields_in_db(&config, &mut db_fields_handle),
            update_issue_link_types_in_db(&config, &mut db_link_types_handles),
            update_project_list_in_db(&config, &mut db_project_list_handle)
    );
}

async fn background_project_update(config: Config, mut db_conn: Pool<Sqlite>) {
  let wait_before_loop_iteration = Duration::from_secs(90);

  loop {
    update_jira_schema(&config, &db_conn).await;
    update_interesting_projects_in_db(&config, &mut db_conn).await;
    tokio::time::sleep(wait_before_loop_iteration).await;
  }
}

async fn background_full_initialise_project(config: Config, mut db_conn: Pool<Sqlite>) {
  let wait_before_loop_iteration = Duration::from_secs(7200);

  loop {
    update_jira_schema(&config, &db_conn).await;
    initialise_interesting_projects_in_db(&config, &mut db_conn).await;
    tokio::time::sleep(wait_before_loop_iteration).await;
  }
}

async fn background_tasks(config: Config, mut db_conn: Pool<Sqlite>) {
  let full_initialise_project = tokio::spawn(background_full_initialise_project(config.clone(), db_conn.clone()));
  let project_update_handle = tokio::spawn(background_project_update(config.clone(), db_conn.clone()));

  let _ = project_update_handle.await;
  let _ = full_initialise_project.await;
}

pub(crate)
async fn server_request_loop(config: &Config, db_conn: &Pool<Sqlite>) {

  let background_tasks_handle = tokio::spawn(background_tasks(config.clone(), db_conn.clone()));

  let (request_to_processor_sender, request_receiver) = tokio::sync::mpsc::channel(1000);
  let (reply_sender, mut reply_receiver) = tokio::sync::mpsc::channel(1000);

  let event_processor_handle = tokio::spawn(process_events(config.clone(), request_receiver, reply_sender, db_conn.clone()));

  let (request_on_stdin_sender, mut request_on_stdin_receiver) = tokio::sync::mpsc::channel(1000);
  let stdin_to_req_handle = std::thread::spawn(move || {
    stdin_to_request(request_on_stdin_sender)
  });

  eprintln!("Ready to accept requests");

  while !reply_receiver.is_closed() {
    tokio::select! {
      req = request_on_stdin_receiver.recv() => {
        match req {
          None => {},
          Some(req) => { let _ = request_to_processor_sender.try_send(req); }
        }
      },
      reply = reply_receiver.recv() => {
        match reply {
          None => {},
          Some(reply) => { print!("{}", reply.0) }
        }
      }
    }
  }

  if !reply_receiver.is_empty() {
    while let Ok(reply) = reply_receiver.try_recv() {
      print!("{}", reply.0)
    }
  }

  request_on_stdin_receiver.close();
  let _ = event_processor_handle.abort();
  drop(stdin_to_req_handle);

  let _ = background_tasks_handle.abort();
}