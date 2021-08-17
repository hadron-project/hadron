//! Hadron client code.

use anyhow::{bail, Context, Result};
use hadron::{Client, ClientCreds};

const ENV_HADRON_URL: &str = "HADRON_URL";
const ENV_HADRON_TOKEN: &str = "HADRON_TOKEN";
const ENV_HADRON_USER: &str = "HADRON_USER";
const ENV_HADRON_PASSWORD: &str = "HADRON_PASSWORD";

/// Create a new Hadron client instance based on the CLI environment.
///
/// The client will be constructed using data from the CLI, falling back to environment variables
/// if CLI options are not specified.
#[tracing::instrument(level = "debug")]
pub async fn new_client(url: Option<&str>, token: Option<&str>, user: Option<&str>, password: Option<&str>) -> Result<Client> {
    let env_url_opt = get_env_opt(ENV_HADRON_URL)?;
    let url = url
        .map(String::from)
        .or(env_url_opt)
        .context("the URL of the Hadron cluster must be provided via HADRON_URL env var or --url CLI option")?;

    let env_token_opt = get_env_opt(ENV_HADRON_TOKEN)?;
    let token = token.map(String::from).or(env_token_opt);

    let env_user_opt = get_env_opt(ENV_HADRON_USER)?;
    let user = user.map(String::from).or(env_user_opt);

    let env_password_opt = get_env_opt(ENV_HADRON_PASSWORD)?;
    let password = password.map(String::from).or(env_password_opt);

    if token.is_none() && user.is_none() {
        bail!("no credentials provided; set credentials via HADRON_TOKEN, HADRON_USER, HADRON_PASSWORD or via their corresponding CLI options");
    }

    // Build a new client and attach the corresponding credentials.
    let creds = match (token, user) {
        (None, None) => {
            bail!("no credentials provided; set credentials via HADRON_TOKEN, HADRON_USER, HADRON_PASSWORD or via their corresponding CLI options")
        }
        (Some(token), None) | (Some(token), Some(_)) => ClientCreds::new_with_token(&token)?,
        (None, Some(user)) => ClientCreds::new_with_password(&user, password.as_deref().unwrap_or(""))?,
    };
    let client = Client::new(url, creds).await?;
    Ok(client)
}

/// Get an env var, returning an option if it does not exist.
fn get_env_opt(key: &str) -> Result<Option<String>> {
    match std::env::var(key) {
        Ok(val) => Ok(Some(val)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(err) => Err(err).with_context(|| format!("malformed env var {}", key)),
    }
}
