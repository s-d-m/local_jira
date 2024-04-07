pub(crate) const DEFAULT_CONFIG_FILE_PATH: &'static str = "/home/sam/.config/local_jira/local_jira.toml";
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

"##;