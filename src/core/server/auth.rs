//! Authentication operations for project APIs.
//!
//! Provides login, logout, and status checking without exposing
//! the underlying HTTP client or keychain implementation.

use super::http::ApiClient;
use crate::error::Result;
use crate::keychain;
use crate::project;
use base64::Engine;
use serde::Serialize;
use std::collections::HashMap;

const PROFILE_KIND: &str = "kind";
const PROFILE_USERNAME: &str = "username";
const PROFILE_PASSWORD: &str = "password";
const PROFILE_TOKEN: &str = "token";

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

#[derive(Debug, Serialize)]
pub struct ProfileSetResult {
    pub profile: String,
    pub kind: String,
    pub stored: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileStatusResult {
    pub profile: String,
    pub kind: Option<String>,
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileRemoveResult {
    pub profile: String,
    pub removed: usize,
}

pub fn profile_scope(profile: &str) -> String {
    format!("profile:{}", profile)
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

pub fn set_profile_basic(
    profile: &str,
    username: &str,
    password: &str,
) -> Result<ProfileSetResult> {
    let scope = profile_scope(profile);
    keychain::set(&scope, PROFILE_KIND, "basic")?;
    keychain::set(&scope, PROFILE_USERNAME, username)?;
    keychain::set(&scope, PROFILE_PASSWORD, password)?;
    Ok(ProfileSetResult {
        profile: profile.to_string(),
        kind: "basic".to_string(),
        stored: true,
    })
}

pub fn set_profile_bearer(profile: &str, token: &str) -> Result<ProfileSetResult> {
    let scope = profile_scope(profile);
    keychain::set(&scope, PROFILE_KIND, "bearer")?;
    keychain::set(&scope, PROFILE_TOKEN, token)?;
    Ok(ProfileSetResult {
        profile: profile.to_string(),
        kind: "bearer".to_string(),
        stored: true,
    })
}

pub fn profile_status(profile: &str) -> Result<ProfileStatusResult> {
    let scope = profile_scope(profile);
    let kind = keychain::get(&scope, PROFILE_KIND)?;
    let available = match kind.as_deref() {
        Some("basic") => {
            keychain::get(&scope, PROFILE_USERNAME)?.is_some()
                && keychain::get(&scope, PROFILE_PASSWORD)?.is_some()
        }
        Some("bearer") => keychain::get(&scope, PROFILE_TOKEN)?.is_some(),
        _ => false,
    };

    Ok(ProfileStatusResult {
        profile: profile.to_string(),
        kind,
        available,
    })
}

pub fn remove_profile(profile: &str) -> Result<ProfileRemoveResult> {
    let scope = profile_scope(profile);
    let variables = vec![
        PROFILE_KIND.to_string(),
        PROFILE_USERNAME.to_string(),
        PROFILE_PASSWORD.to_string(),
        PROFILE_TOKEN.to_string(),
    ];
    let removed = keychain::remove_many(&scope, &variables)?;
    Ok(ProfileRemoveResult {
        profile: profile.to_string(),
        removed,
    })
}

pub fn profile_authorization_header(profile: &str) -> Result<String> {
    let scope = profile_scope(profile);
    let kind =
        keychain::get(&scope, PROFILE_KIND)?.ok_or_else(|| missing_profile_error(profile))?;
    match kind.as_str() {
        "basic" => {
            let username = keychain::get(&scope, PROFILE_USERNAME)?
                .ok_or_else(|| missing_profile_error(profile))?;
            let password = keychain::get(&scope, PROFILE_PASSWORD)?
                .ok_or_else(|| missing_profile_error(profile))?;
            let encoded = base64::engine::general_purpose::STANDARD
                .encode(format!("{}:{}", username, password));
            Ok(format!("Basic {}", encoded))
        }
        "bearer" => {
            let token = keychain::get(&scope, PROFILE_TOKEN)?
                .ok_or_else(|| missing_profile_error(profile))?;
            Ok(format!("Bearer {}", token))
        }
        other => Err(crate::error::Error::validation_invalid_argument(
            "auth-profile",
            format!("Unsupported auth profile kind '{}'", other),
            Some(profile.to_string()),
            Some(vec!["basic".to_string(), "bearer".to_string()]),
        )),
    }
}

fn missing_profile_error(profile: &str) -> crate::error::Error {
    crate::error::Error::new(
        crate::error::ErrorCode::ExtensionNotFound,
        format!("Auth profile '{}' is not set", profile),
        serde_json::Value::Null,
    )
    .with_hint(format!(
        "Run 'homeboy auth profile set-basic {} --username <user>' or 'homeboy auth profile set-bearer {}'",
        profile, profile
    ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::VariableSource;

    #[test]
    fn test_redact() {
        assert_eq!(redact("secret"), "********");
        assert_eq!(redact(""), "");
    }

    #[test]
    fn test_variable_available_config() {
        let source = VariableSource {
            source: "config".to_string(),
            value: Some("value".to_string()),
            env_var: None,
        };

        assert!(variable_available("project", "token", &source));
    }

    #[test]
    fn test_variable_available_missing_config() {
        let source = VariableSource {
            source: "config".to_string(),
            value: None,
            env_var: None,
        };

        assert!(!variable_available("project", "token", &source));
    }

    #[test]
    fn test_variable_available_unknown_source() {
        let source = VariableSource {
            source: "unknown".to_string(),
            value: None,
            env_var: None,
        };

        assert!(!variable_available("project", "token", &source));
    }

    #[test]
    #[ignore]
    fn test_login() {
        let credentials = HashMap::new();
        let _ = login("homeboy-auth-test", credentials);
    }

    #[test]
    #[ignore]
    fn test_logout() {
        let _ = logout("homeboy-auth-test");
    }

    #[test]
    #[ignore]
    fn test_set() {
        let result = set("homeboy-auth-test", "token", "secret-value").expect("store value");

        assert!(result.stored);
        assert_eq!(result.project_id, "homeboy-auth-test");
        assert_eq!(result.variable, "token");
        remove("homeboy-auth-test", "token").expect("cleanup value");
    }

    #[test]
    #[ignore]
    fn test_get() {
        set("homeboy-auth-test", "token", "secret-value").expect("store value");
        let result = get("homeboy-auth-test", "token", true).expect("read value");

        assert_eq!(result.value.as_deref(), Some("********"));
        assert!(result.redacted);
        remove("homeboy-auth-test", "token").expect("cleanup value");
    }

    #[test]
    #[ignore]
    fn test_remove() {
        set("homeboy-auth-test", "token", "secret-value").expect("store value");
        let result = remove("homeboy-auth-test", "token").expect("remove value");

        assert!(result.removed);
    }

    #[test]
    #[ignore]
    fn test_status() {
        let _ = status("homeboy-auth-test");
    }
}
