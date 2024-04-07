PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000; -- Release lock after 5 seconds

BEGIN;

CREATE TABLE IF NOT EXISTS people (
   accountId TEXT UNIQUE PRIMARY KEY NOT NULL,
   displayName TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
   jira_id INTEGER PRIMARY KEY NOT NULL,
   key TEXT UNIQUE NOT NULL,
   name TEXT NOT NULL,
   lead_id TEXT,
   FOREIGN KEY(lead_id) REFERENCES people(accountId)
);

CREATE INDEX IF NOT EXISTS projects_key ON projects(key);

CREATE TABLE IF NOT EXISTS fields_name (
  jira_field_name TEXT PRIMARY KEY NOT NULL,
  human_name TEXT NOT NULL
);

COMMIT;