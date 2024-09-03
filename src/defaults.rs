pub(crate) const DEFAULT_CONFIG_FILE_PATH: &'static str = "local_jira/local_jira.toml";
pub(crate) const DEFAULT_DB_NAME: &'static str = "local_jira.sqlite";
pub(crate) const JIRA_API_TOKEN_ENV_VAR: &'static str = "JIRA_API_TOKEN";

pub(crate) const EXAMPLE_CONFIG_FILE: &'static str =
r##"# Example configuration file

server_address = "https://<acme-company.com>:80"
user_login = "Your.Name@acme-company.com" # likely your company's email address
api_token = "<API TOKEN>" # see jira documentation to find out how to export it
# alternatively use JIRA_API_TOKEN=<API_TOKEN> as environment variable to pass the token

# Path to the database. When not given, a default path is used that is a file named local_jira.sqlite
# in the same folder as the configuration file.
local_database = "path to the database file"

# List of jira project keys you are interesting in. This is the name part in the jira ticket
# numbers. E.g. for a jira ticket COMPANYPROJECT-1234, the project key is COMPANYPROJECT
interesting_projects = [ "PRJKEYONE", "PRJKEYTWO", "PRJKEYTHREE" ]

# Unfortunately jira doesn't provide an option to download ticket attachments through
# an API using the JIRA_API_TOKEN. At least I didn't find a solution.
#  The workaround is instead to ask the user to log
# into jira using firefox, and provide the path to the cookie file containing the tenant
# session cookie. local_jira will retrieve that cookie and download attachment files
# with it. Without this cookie, No attachment file will be downloaded.
mozilla_cookies_db = "/Path/to/Mozilla/Firefox/Profiles/<profile key>/cookies.sqlite"
"##;