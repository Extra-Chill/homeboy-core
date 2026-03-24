//! grammar_loading — extracted from grammar.rs.

use std::path::Path;
use crate::engine::local_files;
use crate::error::{Error, Result};
use std::path::Path;
use crate::error::Result;
use super::load;
use super::Grammar;
use super::super::*;


/// Load a grammar from a TOML file.
pub fn load_grammar(path: &Path) -> Result<Grammar> {
    let content = local_files::read_file(path, "read grammar file")?;
    toml::from_str(&content).map_err(|e| {
        Error::internal_io(
            format!("Failed to parse grammar {}: {}", path.display(), e),
            Some("grammar.load".to_string()),
        )
    })
}

/// Load a grammar from a JSON file.
pub fn load_grammar_json(path: &Path) -> Result<Grammar> {
    let content = local_files::read_file(path, "read grammar file")?;
    serde_json::from_str(&content).map_err(|e| {
        Error::internal_io(
            format!("Failed to parse grammar {}: {}", path.display(), e),
            Some("grammar.load".to_string()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_load_grammar_default_path() {
        let path = Path::new("");
        let _result = load_grammar(&path);
    }

    #[test]
    fn test_load_grammar_some_grammar_load_to_string() {
        let path = Path::new("");
        let _result = load_grammar(&path);
    }

    #[test]
    fn test_load_grammar_json_default_path() {
        let path = Path::new("");
        let _result = load_grammar_json(&path);
    }

    #[test]
    fn test_load_grammar_json_some_grammar_load_to_string() {
        let path = Path::new("");
        let _result = load_grammar_json(&path);
    }

}
