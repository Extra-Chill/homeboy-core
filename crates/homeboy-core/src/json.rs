use crate::{Error, Result};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub fn read_json_file(path: impl AsRef<Path>) -> Result<Value> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

pub fn write_json_file_pretty(path: impl AsRef<Path>, value: &Value) -> Result<()> {
    let path = path.as_ref();
    let content = serde_json::to_string_pretty(value)?;
    write_file_atomic(path, content.as_bytes())
}

pub fn set_json_pointer(root: &mut Value, pointer: &str, new_value: Value) -> Result<()> {
    let pointer = normalize_pointer(pointer)?;
    let Some((parent_ptr, token)) = split_parent_pointer(&pointer) else {
        *root = new_value;
        return Ok(());
    };

    let parent = ensure_pointer_container(root, &parent_ptr)?;
    set_child(parent, &token, new_value)
}

pub fn remove_json_pointer(root: &mut Value, pointer: &str) -> Result<()> {
    let pointer = normalize_pointer(pointer)?;
    let Some((parent_ptr, token)) = split_parent_pointer(&pointer) else {
        return Err(Error::Config(
            "Cannot remove the root JSON value".to_string(),
        ));
    };

    let Some(parent) = root.pointer_mut(&parent_ptr) else {
        return Err(Error::Config(format!(
            "JSON pointer parent path not found: {}",
            parent_ptr
        )));
    };

    remove_child(parent, &token)
}

pub fn set_json_value_in_file(
    path: impl AsRef<Path>,
    pointer: &str,
    new_value: Value,
) -> Result<()> {
    let path = path.as_ref();
    let mut json = read_json_file(path)?;
    set_json_pointer(&mut json, pointer, new_value)?;
    write_json_file_pretty(path, &json)
}

fn write_file_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Config(format!("Invalid path: {}", path.display())))?;

    let filename = path
        .file_name()
        .ok_or_else(|| Error::Config(format!("Invalid path: {}", path.display())))?;

    let tmp_path: PathBuf = parent.join(format!("{}.tmp", filename.to_string_lossy()));

    fs::write(&tmp_path, content)?;
    fs::rename(&tmp_path, path)?;

    Ok(())
}

fn normalize_pointer(pointer: &str) -> Result<String> {
    if pointer.is_empty() {
        return Ok(String::new());
    }

    if pointer == "/" {
        return Err(Error::Config("Invalid JSON pointer '/'".to_string()));
    }

    if !pointer.starts_with('/') {
        return Err(Error::Config(format!(
            "JSON pointer must start with '/': {}",
            pointer
        )));
    }

    Ok(pointer.to_string())
}

fn split_parent_pointer(pointer: &str) -> Option<(String, String)> {
    if pointer.is_empty() {
        return None;
    }

    let mut parts = pointer.rsplitn(2, '/');
    let token = parts.next()?.to_string();
    let parent = parts.next().unwrap_or("");

    let parent_ptr = if parent.is_empty() {
        String::new()
    } else {
        parent.to_string()
    };

    Some((parent_ptr, unescape_token(&token)))
}

fn ensure_pointer_container<'a>(root: &'a mut Value, pointer: &str) -> Result<&'a mut Value> {
    if pointer.is_empty() {
        return Ok(root);
    }

    let tokens: Vec<String> = pointer.split('/').skip(1).map(unescape_token).collect();

    let mut current = root;

    for token in tokens {
        let next = match current {
            Value::Object(map) => map
                .entry(token)
                .or_insert_with(|| Value::Object(serde_json::Map::new())),
            Value::Null => {
                *current = Value::Object(serde_json::Map::new());
                if let Value::Object(map) = current {
                    map.entry(token)
                        .or_insert_with(|| Value::Object(serde_json::Map::new()))
                } else {
                    unreachable!()
                }
            }
            Value::Array(arr) => {
                let index = parse_index(&token)?;
                if index >= arr.len() {
                    return Err(Error::Config(format!(
                        "Array index out of bounds while creating path: {}",
                        pointer
                    )));
                }
                &mut arr[index]
            }
            _ => {
                return Err(Error::Config(format!(
                    "Expected object/array at pointer '{}', found {}",
                    pointer,
                    value_type_name(current)
                )))
            }
        };

        current = next;
    }

    Ok(current)
}

fn set_child(parent: &mut Value, token: &str, value: Value) -> Result<()> {
    match parent {
        Value::Object(map) => {
            map.insert(token.to_string(), value);
            Ok(())
        }
        Value::Array(arr) => {
            let index = parse_index(token)?;
            if index >= arr.len() {
                return Err(Error::Config(format!(
                    "Array index out of bounds: {}",
                    index
                )));
            }
            arr[index] = value;
            Ok(())
        }
        _ => Err(Error::Config(format!(
            "Cannot set child on {}",
            value_type_name(parent)
        ))),
    }
}

fn remove_child(parent: &mut Value, token: &str) -> Result<()> {
    match parent {
        Value::Object(map) => {
            map.remove(token);
            Ok(())
        }
        Value::Array(arr) => {
            let index = parse_index(token)?;
            if index >= arr.len() {
                return Err(Error::Config(format!(
                    "Array index out of bounds: {}",
                    index
                )));
            }
            arr.remove(index);
            Ok(())
        }
        _ => Err(Error::Config(format!(
            "Cannot remove child on {}",
            value_type_name(parent)
        ))),
    }
}

fn parse_index(token: &str) -> Result<usize> {
    token
        .parse::<usize>()
        .map_err(|_| Error::Config(format!("Invalid array index token: {}", token)))
}

fn unescape_token(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn value_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
