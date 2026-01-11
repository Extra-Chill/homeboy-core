use crate::{Error, Result};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigValueType {
    String,
    StringArray,
}

#[derive(Debug, Clone, Copy)]
pub struct ConfigKey {
    pub name: &'static str,
    pub pointer: &'static str,
    pub value_type: ConfigValueType,
}

pub const KNOWN_CONFIG_KEYS: &[ConfigKey] = &[
    ConfigKey {
        name: "activeProjectId",
        pointer: "/activeProjectId",
        value_type: ConfigValueType::String,
    },
    ConfigKey {
        name: "defaultChangelogNextSectionLabel",
        pointer: "/defaultChangelogNextSectionLabel",
        value_type: ConfigValueType::String,
    },
    ConfigKey {
        name: "defaultChangelogNextSectionAliases",
        pointer: "/defaultChangelogNextSectionAliases",
        value_type: ConfigValueType::StringArray,
    },
];

pub fn find_config_key(name: &str) -> Option<ConfigKey> {
    for key in KNOWN_CONFIG_KEYS {
        if key.name == name {
            return Some(*key);
        }
    }
    None
}

pub fn find_config_key_by_pointer(pointer: &str) -> Option<ConfigKey> {
    for key in KNOWN_CONFIG_KEYS {
        if key.pointer == pointer {
            return Some(*key);
        }
    }
    None
}

pub fn parse_config_value(value_type: ConfigValueType, raw: &str) -> Result<Value> {
    match value_type {
        ConfigValueType::String => Ok(Value::String(raw.to_string())),
        ConfigValueType::StringArray => {
            let items: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect();

            if items.is_empty() {
                return Err(Error::validation_invalid_argument(
                    "value",
                    "String array value cannot be empty",
                    None,
                    None,
                ));
            }

            Ok(Value::Array(items.into_iter().map(Value::String).collect()))
        }
    }
}
