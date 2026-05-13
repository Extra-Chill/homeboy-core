//! Authentication operations for project APIs.
//!
//! Provides login, logout, and status checking without exposing
//! the underlying HTTP client or keychain implementation.

use super::http::ApiClient;
use crate::error::Result;
use crate::keychain;
use crate::project;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct LoginResult {
    pub project_id: String,
    pub success: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub project_id: String,
    pub authenticated: bool,
    pub variables: Vec<AuthVariableStatus>,
}

#[derive(Debug, Serialize)]
pub struct LogoutResult {
    pub project_id: String,
    pub removed: usize,
}

#[derive(Debug, Serialize)]
pub struct SetResult {
    pub project_id: String,
    pub variable: String,
    pub stored: bool,
}

#[derive(Debug, Serialize)]
pub struct GetResult {
    pub project_id: String,
    pub variable: String,
    pub value: Option<String>,
    pub redacted: bool,
}

#[derive(Debug, Serialize)]
pub struct RemoveResult {
    pub project_id: String,
    pub variable: String,
    pub removed: bool,
}

#[derive(Debug, Serialize)]
pub struct AuthVariableStatus {
    pub name: String,
    pub source: String,
    pub available: bool,
}

/// Authenticates with a project's API using provided credentials.
///
/// The caller is responsible for obtaining credentials (prompting, flags, etc.).
/// This function handles the authentication flow and token storage.
pub fn login(project_id: &str, credentials: HashMap<String, String>) -> Result<LoginResult> {
    let project = project::load(project_id)?;
    let client = ApiClient::new(project_id, &project.api)?;
    client.login(&credentials)?;

    Ok(LoginResult {
        project_id: project_id.to_string(),
        success: true,
    })
}

/// Clears stored authentication for a project.
pub fn logout(project_id: &str) -> Result<LogoutResult> {
    let project = project::load(project_id)?;
    let variable_names = keychain_variable_names(&project);
    let removed = keychain::remove_many(project_id, &variable_names)?;

    Ok(LogoutResult {
        project_id: project_id.to_string(),
        removed,
    })
}

/// Stores a project API variable in the keychain.
pub fn set(project_id: &str, variable: &str, value: &str) -> Result<SetResult> {
    keychain::set(project_id, variable, value)?;
    Ok(SetResult {
        project_id: project_id.to_string(),
        variable: variable.to_string(),
        stored: true,
    })
}

/// Retrieves a project API variable from the keychain.
pub fn get(project_id: &str, variable: &str, redacted: bool) -> Result<GetResult> {
    let value =
        keychain::get(project_id, variable)?.map(
            |value| {
                if redacted {
                    redact(&value)
                } else {
                    value
                }
            },
        );

    Ok(GetResult {
        project_id: project_id.to_string(),
        variable: variable.to_string(),
        value,
        redacted,
    })
}

/// Removes a project API variable from the keychain.
pub fn remove(project_id: &str, variable: &str) -> Result<RemoveResult> {
    let removed = keychain::get(project_id, variable)?.is_some();
    keychain::remove(project_id, variable)?;

    Ok(RemoveResult {
        project_id: project_id.to_string(),
        variable: variable.to_string(),
        removed,
    })
}

/// Checks authentication status for a project.
pub fn status(project_id: &str) -> Result<AuthStatus> {
    let project = project::load(project_id)?;
    let client = ApiClient::new(project_id, &project.api)?;

    Ok(AuthStatus {
        project_id: project_id.to_string(),
        authenticated: client.is_authenticated(),
        variables: variable_statuses(project_id, &project),
    })
}

fn keychain_variable_names(project: &project::Project) -> Vec<String> {
    project
        .api
        .auth
        .as_ref()
        .map(|auth| {
            auth.variables
                .iter()
                .filter_map(|(name, source)| {
                    (source.source == "keychain").then(|| name.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn variable_statuses(project_id: &str, project: &project::Project) -> Vec<AuthVariableStatus> {
    let Some(auth) = project.api.auth.as_ref() else {
        return Vec::new();
    };

    auth.variables
        .iter()
        .map(|(name, source)| AuthVariableStatus {
            name: name.to_string(),
            source: source.source.clone(),
            available: variable_available(project_id, name, source),
        })
        .collect()
}

fn variable_available(project_id: &str, name: &str, source: &project::VariableSource) -> bool {
    match source.source.as_str() {
        "config" => source.value.is_some(),
        "env" => {
            let default_env = name.to_string();
            let env_var = source.env_var.as_ref().unwrap_or(&default_env);
            std::env::var(env_var).is_ok()
        }
        "keychain" => keychain::exists(project_id, name),
        _ => false,
    }
}

fn redact(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    "********".to_string()
}
