PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000; -- Release lock after 5 seconds

BEGIN;

CREATE TABLE IF NOT EXISTS people (
   accountId TEXT UNIQUE PRIMARY KEY NOT NULL,
   displayName TEXT UNIQUE NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   key TEXT UNIQUE NOT NULL, -- something like COMPANYPROJ
   name TEXT NOT NULL,
   lead_id TEXT,
   FOREIGN KEY(lead_id) REFERENCES people(accountId)
);

CREATE INDEX IF NOT EXISTS projects_key ON projects(key);

CREATE TABLE IF NOT EXISTS fields_name (
  jira_field_name TEXT UNIQUE PRIMARY KEY NOT NULL, -- like customfield_12345
  human_name TEXT NOT NULL                   -- like country / vendor / supplier...
);

CREATE TABLE IF NOT EXISTS IssueType (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   name TEXT UNIQUE NOT NULL,
   description TEXT
);

CREATE TABLE IF NOT EXISTS issue (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   key TEXT UNIQUE NOT NULL,  -- something like COMPANYPROJ-1234
   IssueType INTEGER,
   FOREIGN KEY (IssueType) REFERENCES IssueType(jira_id)
);

CREATE TABLE IF NOT EXISTS link_type (
   jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
   name TEXT NOT NULL,
   outward_name TEXT NOT NULL,
   inward_name TEXT NOT NULL
);

-- link between two issues (aka between two COMPANYPROJ-XXXXX)
CREATE TABLE IF NOT EXISTS issuelink (
    jira_id INTEGER UNIQUE PRIMARY KEY NOT NULL,
    link_type_id INTEGER,
    outward_link INTEGER,
    inward_link INTEGER,
    FOREIGN KEY(link_type_id) REFERENCES link_type(jira_id),
    FOREIGN KEY(outward_link) REFERENCES issue(jira_id),
    FOREIGN KEY(inward_link) REFERENCES issue(jira_id),
    CHECK (outward_link != inward_link)
);

CREATE TABLE IF NOT EXISTS watcher (
    person TEXT,
    issue INTEGER,
    FOREIGN KEY (person) REFERENCES people(accountId),
    FOREIGN KEY (issue) REFERENCES issue(jira_id)
);

COMMIT;