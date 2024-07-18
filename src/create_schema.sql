PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000; -- Release lock after 5 seconds
PRAGMA journal_mode = WAL;
PRAGMA case_sensitive_like = ON;

BEGIN;

CREATE TABLE IF NOT EXISTS people (
   accountId TEXT UNIQUE PRIMARY KEY NOT NULL,
   displayName TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS Project (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   key TEXT UNIQUE NOT NULL,
   name TEXT,
   description TEXT,
   is_archived INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS projects_key ON Project(key);

CREATE TABLE IF NOT EXISTS Field (
  jira_id TEXT UNIQUE PRIMARY KEY NOT NULL, -- like customfield_12345
  key TEXT NOT NULL,
  human_name TEXT NOT NULL,                   -- like country / vendor / supplier...
  schema TEXT NOT NULL,
  is_custom INTEGER
);

CREATE TABLE IF NOT EXISTS IssueType (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   name TEXT NOT NULL,
   description TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS IssueTypePerProject (
   project_id INTEGER,
   issue_type_id INTEGER,
   FOREIGN KEY(project_id) REFERENCES Project(jira_id),
   FOREIGN KEY(issue_type_id) REFERENCES IssueType(jira_id),
   UNIQUE(project_id, issue_type_id)
);

CREATE TABLE IF NOT EXISTS Issue (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   key TEXT UNIQUE NOT NULL,  -- something like COMPANYPROJ-1234
   project_key TEXT NOT NULL,
   FOREIGN KEY (project_key) REFERENCES Project(key),
   UNIQUE(key, project_key)
);

CREATE INDEX IF NOT EXISTS issue_key ON Issue(key);

CREATE TABLE IF NOT EXISTS IssueLinkType (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   name TEXT NOT NULL,
   outward_name TEXT NOT NULL,
   inward_name TEXT NOT NULL
);

-- link between two issues (aka between two COMPANYPROJ-XXXXX)
CREATE TABLE IF NOT EXISTS IssueLink (
    jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
    link_type_id INTEGER,
    outward_issue_id INTEGER,
    inward_issue_id INTEGER,
    FOREIGN KEY(link_type_id) REFERENCES IssueLinkType(jira_id),
    FOREIGN KEY(outward_issue_id) REFERENCES Issue(jira_id),
    FOREIGN KEY(inward_issue_id) REFERENCES Issue(jira_id),
    CHECK (outward_issue_id != inward_issue_id)
);

CREATE TABLE IF NOT EXISTS IssueField (
   issue_id INTEGER,
   field_id STRING,
   field_value STRING,

   FOREIGN KEY(issue_id) REFERENCES Issue(jira_id),
   FOREIGN KEY(field_id) REFERENCES Field(jira_id),
   UNIQUE(issue_id, field_id)
);

CREATE TABLE IF NOT EXISTS watcher (
    person TEXT,
    Issue INTEGER,
    FOREIGN KEY (person) REFERENCES people(accountId),
    FOREIGN KEY (Issue) REFERENCES Issue(jira_id)
);

CREATE TABLE IF NOT EXISTS Attachment (
  uuid TEXT UNIQUE,
  id INTEGER UNIQUE PRIMARY KEY NOT NULL,
  issue_id INTEGER NOT NULL,
  filename TEXT NOT NULL,
  mime_type TEXT,
  file_size INT NOT NULL,
  content_data BLOB,

  FOREIGN KEY (issue_id) REFERENCES Issue(jira_id)
);

COMMIT;