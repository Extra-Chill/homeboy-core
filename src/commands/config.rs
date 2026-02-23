use clap::{Args, Subcommand};
use serde::Serialize;
use serde_json::Value;

use homeboy::defaults::{self, Defaults, HomeboyConfig};

use super::CmdResult;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Subcommand)]
enum ConfigCommand {
    /// Display configuration (merged defaults + file)
    Show {
        /// Show only built-in defaults (ignore homeboy.json)
        #[arg(long)]
        builtin: bool,
    },
    /// Set a configuration value at a JSON pointer path
    Set {
        /// JSON pointer path (e.g., /defaults/deploy/scp_flags)
        pointer: String,
        /// Value to set (JSON)
        value: String,
    },
    /// Remove a configuration value at a JSON pointer path
    Remove {
        /// JSON pointer path (e.g., /defaults/deploy/scp_flags)
        pointer: String,
    },
    /// Reset configuration to built-in defaults (deletes homeboy.json)
    Reset,
    /// Show the path to homeboy.json
    Path,
}

#[derive(Debug, Serialize)]
pub struct ConfigOutput {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<HomeboyConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    defaults: Option<Defaults>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exists: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pointer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    deleted: Option<bool>,
}

pub fn run(args: ConfigArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<ConfigOutput> {
    match args.command {
        ConfigCommand::Show { builtin } => show(builtin),
        ConfigCommand::Set { pointer, value } => set(&pointer, &value),
        ConfigCommand::Remove { pointer } => remove(&pointer),
        ConfigCommand::Reset => reset(),
        ConfigCommand::Path => path(),
    }
}

fn show(builtin: bool) -> CmdResult<ConfigOutput> {
    if builtin {
        Ok((
            ConfigOutput {
                command: "config.show".to_string(),
                defaults: Some(defaults::builtin_defaults()),
                config: None,
                path: None,
                exists: None,
                pointer: None,
                value: None,
                deleted: None,
            },
            0,
        ))
    } else {
        let config = defaults::load_config();
        Ok((
            ConfigOutput {
                command: "config.show".to_string(),
                config: Some(config),
                defaults: None,
                path: None,
                exists: None,
                pointer: None,
                value: None,
                deleted: None,
            },
            0,
        ))
    }
}

fn set(pointer: &str, value_str: &str) -> CmdResult<ConfigOutput> {
    // Validate pointer format
    if !pointer.starts_with('/') {
        return Err(homeboy::Error::validation_invalid_argument(
            "pointer",
            "JSON pointer must start with '/'",
            None,
            None,
        ));
    }

    // Parse the value as JSON
    let value: Value = serde_json::from_str(value_str).map_err(|e| {
        homeboy::Error::validation_invalid_json(
            e,
            Some("parse value".to_string()),
            Some(value_str.chars().take(200).collect::<String>()),
        )
    })?;

    // Load current config (or create default)
    let mut config = defaults::load_config();

    // Convert to JSON, set the value, convert back
    let mut config_json = serde_json::to_value(&config).map_err(|e| {
        homeboy::Error::internal_unexpected(format!("Failed to serialize config: {}", e))
    })?;

    // Navigate to the pointer location and set the value
    set_json_pointer(&mut config_json, pointer, value.clone())?;

    // Convert back to HomeboyConfig
    config = serde_json::from_value(config_json).map_err(|e| {
        homeboy::Error::validation_invalid_json(e, Some("deserialize config".to_string()), None)
    })?;

    // Save the config
    defaults::save_config(&config)?;

    Ok((
        ConfigOutput {
            command: "config.set".to_string(),
            config: Some(config),
            defaults: None,
            path: None,
            exists: None,
            pointer: Some(pointer.to_string()),
            value: Some(value),
            deleted: None,
        },
        0,
    ))
}

fn remove(pointer: &str) -> CmdResult<ConfigOutput> {
    // Validate pointer format
    if !pointer.starts_with('/') {
        return Err(homeboy::Error::validation_invalid_argument(
            "pointer",
            "JSON pointer must start with '/'",
            None,
            None,
        ));
    }

    // Load current config
    let mut config = defaults::load_config();

    // Convert to JSON
    let mut config_json = serde_json::to_value(&config).map_err(|e| {
        homeboy::Error::internal_unexpected(format!("Failed to serialize config: {}", e))
    })?;

    // Remove the value at the pointer
    remove_json_pointer(&mut config_json, pointer)?;

    // Convert back to HomeboyConfig
    config = serde_json::from_value(config_json).map_err(|e| {
        homeboy::Error::validation_invalid_json(e, Some("deserialize config".to_string()), None)
    })?;

    // Save the config
    defaults::save_config(&config)?;

    Ok((
        ConfigOutput {
            command: "config.remove".to_string(),
            config: Some(config),
            defaults: None,
            path: None,
            exists: None,
            pointer: Some(pointer.to_string()),
            value: None,
            deleted: None,
        },
        0,
    ))
}

fn reset() -> CmdResult<ConfigOutput> {
    let deleted = defaults::reset_config()?;

    Ok((
        ConfigOutput {
            command: "config.reset".to_string(),
            config: None,
            defaults: Some(defaults::builtin_defaults()),
            path: Some(defaults::config_path()?),
            exists: None,
            pointer: None,
            value: None,
            deleted: Some(deleted),
        },
        0,
    ))
}

fn path() -> CmdResult<ConfigOutput> {
    let path = defaults::config_path()?;
    let exists = defaults::config_exists();

    Ok((
        ConfigOutput {
            command: "config.path".to_string(),
            config: None,
            defaults: None,
            path: Some(path),
            exists: Some(exists),
            pointer: None,
            value: None,
            deleted: None,
        },
        0,
    ))
}

/// Set a value at a JSON pointer path, creating intermediate objects as needed.
fn set_json_pointer(root: &mut Value, pointer: &str, value: Value) -> homeboy::Result<()> {
    let parts: Vec<&str> = pointer[1..].split('/').collect();

    if parts.is_empty() {
        *root = value;
        return Ok(());
    }

    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        let key = unescape_json_pointer(part);

        if i == parts.len() - 1 {
            // Last part: set the value
            match current {
                Value::Object(map) => {
                    map.insert(key, value);
                    return Ok(());
                }
                Value::Array(arr) => {
                    let index: usize = key.parse().map_err(|_| {
                        homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Invalid array index: {}", key),
                            None,
                            None,
                        )
                    })?;
                    if index < arr.len() {
                        arr[index] = value;
                    } else if index == arr.len() {
                        arr.push(value);
                    } else {
                        return Err(homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Array index {} out of bounds (length {})", index, arr.len()),
                            None,
                            None,
                        ));
                    }
                    return Ok(());
                }
                _ => {
                    return Err(homeboy::Error::validation_invalid_argument(
                        "pointer",
                        format!("Cannot set property on non-object at path: {}", pointer),
                        None,
                        None,
                    ));
                }
            }
        } else {
            // Intermediate part: navigate or create
            match current {
                Value::Object(map) => {
                    if !map.contains_key(&key) {
                        map.insert(key.clone(), Value::Object(serde_json::Map::new()));
                    }
                    current = map
                        .get_mut(&key)
                        .expect("key just inserted or already exists");
                }
                Value::Array(arr) => {
                    let index: usize = key.parse().map_err(|_| {
                        homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Invalid array index: {}", key),
                            None,
                            None,
                        )
                    })?;
                    if index >= arr.len() {
                        return Err(homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Array index {} out of bounds (length {})", index, arr.len()),
                            None,
                            None,
                        ));
                    }
                    current = &mut arr[index];
                }
                _ => {
                    return Err(homeboy::Error::validation_invalid_argument(
                        "pointer",
                        format!("Cannot navigate through non-object at path: {}", pointer),
                        None,
                        None,
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Remove a value at a JSON pointer path.
fn remove_json_pointer(root: &mut Value, pointer: &str) -> homeboy::Result<()> {
    let parts: Vec<&str> = pointer[1..].split('/').collect();

    if parts.is_empty() {
        return Err(homeboy::Error::validation_invalid_argument(
            "pointer",
            "Cannot remove root element",
            None,
            None,
        ));
    }

    let mut current = root;

    for (i, part) in parts.iter().enumerate() {
        let key = unescape_json_pointer(part);

        if i == parts.len() - 1 {
            // Last part: remove the value
            match current {
                Value::Object(map) => {
                    if map.remove(&key).is_none() {
                        return Err(homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Key '{}' not found", key),
                            None,
                            None,
                        ));
                    }
                    return Ok(());
                }
                Value::Array(arr) => {
                    let index: usize = key.parse().map_err(|_| {
                        homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Invalid array index: {}", key),
                            None,
                            None,
                        )
                    })?;
                    if index >= arr.len() {
                        return Err(homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Array index {} out of bounds (length {})", index, arr.len()),
                            None,
                            None,
                        ));
                    }
                    arr.remove(index);
                    return Ok(());
                }
                _ => {
                    return Err(homeboy::Error::validation_invalid_argument(
                        "pointer",
                        format!("Cannot remove from non-object at path: {}", pointer),
                        None,
                        None,
                    ));
                }
            }
        } else {
            // Intermediate part: navigate
            match current {
                Value::Object(map) => {
                    current = map.get_mut(&key).ok_or_else(|| {
                        homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Key '{}' not found", key),
                            None,
                            None,
                        )
                    })?;
                }
                Value::Array(arr) => {
                    let index: usize = key.parse().map_err(|_| {
                        homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Invalid array index: {}", key),
                            None,
                            None,
                        )
                    })?;
                    if index >= arr.len() {
                        return Err(homeboy::Error::validation_invalid_argument(
                            "pointer",
                            format!("Array index {} out of bounds (length {})", index, arr.len()),
                            None,
                            None,
                        ));
                    }
                    current = &mut arr[index];
                }
                _ => {
                    return Err(homeboy::Error::validation_invalid_argument(
                        "pointer",
                        format!("Cannot navigate through non-object at path: {}", pointer),
                        None,
                        None,
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Unescape JSON pointer special characters (~0 = ~, ~1 = /)
fn unescape_json_pointer(s: &str) -> String {
    s.replace("~1", "/").replace("~0", "~")
}
