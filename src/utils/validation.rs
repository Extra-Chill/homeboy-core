//! Input validation primitives.
//!
//! Provides ergonomic helpers for common validation patterns:
//! - Unwrapping Option values with descriptive errors
//! - Validating non-empty strings and collections
//!
//! These replace verbose ok_or_else + Error::validation_invalid_argument chains.

use crate::error::{Error, Result};

/// Require an Option to contain a value.
///
/// Replaces the common pattern:
/// ```ignore
/// value.ok_or_else(|| Error::validation_invalid_argument("field", "msg", None, None))?
/// ```
///
/// With:
/// ```ignore
/// validation::require(value, "field", "msg")?
/// ```
pub fn require<T>(opt: Option<T>, field: &str, message: &str) -> Result<T> {
    opt.ok_or_else(|| Error::validation_invalid_argument(field, message, None, None))
}

/// Require an Option to contain a value, with hints for resolution.
pub fn require_with_hints<T>(
    opt: Option<T>,
    field: &str,
    message: &str,
    hints: Vec<String>,
) -> Result<T> {
    opt.ok_or_else(|| Error::validation_invalid_argument(field, message, None, Some(hints)))
}

/// Require a string to be non-empty after trimming.
///
/// Returns a reference to the trimmed string on success.
pub fn require_non_empty<'a>(value: &'a str, field: &str, message: &str) -> Result<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(Error::validation_invalid_argument(field, message, None, None))
    } else {
        Ok(trimmed)
    }
}

/// Require a collection to be non-empty.
pub fn require_non_empty_vec<'a, T>(vec: &'a [T], field: &str, message: &str) -> Result<&'a [T]> {
    if vec.is_empty() {
        Err(Error::validation_invalid_argument(field, message, None, None))
    } else {
        Ok(vec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_returns_value_when_some() {
        let result = require(Some("value"), "field", "msg");
        assert_eq!(result.unwrap(), "value");
    }

    #[test]
    fn require_returns_error_when_none() {
        let result: Result<&str> = require(None, "field", "Missing field");
        assert!(result.is_err());
    }

    #[test]
    fn require_non_empty_passes_for_non_empty() {
        let result = require_non_empty("hello", "field", "msg");
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn require_non_empty_trims_whitespace() {
        let result = require_non_empty("  hello  ", "field", "msg");
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn require_non_empty_fails_for_empty() {
        let result = require_non_empty("", "field", "Cannot be empty");
        assert!(result.is_err());
    }

    #[test]
    fn require_non_empty_fails_for_whitespace_only() {
        let result = require_non_empty("   ", "field", "Cannot be empty");
        assert!(result.is_err());
    }

    #[test]
    fn require_non_empty_vec_passes_for_non_empty() {
        let vec = vec![1, 2, 3];
        let result = require_non_empty_vec(&vec, "field", "msg");
        assert_eq!(result.unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn require_non_empty_vec_fails_for_empty() {
        let vec: Vec<i32> = vec![];
        let result = require_non_empty_vec(&vec, "field", "Cannot be empty");
        assert!(result.is_err());
    }
}
