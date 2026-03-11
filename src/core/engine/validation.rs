//! Input validation primitives.

use crate::error::{Error, Result};

pub fn require<T>(opt: Option<T>, field: &str, message: &str) -> Result<T> {
    opt.ok_or_else(|| Error::validation_invalid_argument(field, message, None, None))
}

pub fn require_with_hints<T>(
    opt: Option<T>,
    field: &str,
    message: &str,
    hints: Vec<String>,
) -> Result<T> {
    opt.ok_or_else(|| Error::validation_invalid_argument(field, message, None, Some(hints)))
}

pub struct ValidationCollector {
    errors: Vec<crate::error::ValidationErrorItem>,
}

impl ValidationCollector {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn capture<T>(&mut self, result: Result<T>, field: &str) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                let problem = err
                    .details
                    .get("problem")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| err.message.clone());

                self.errors.push(crate::error::ValidationErrorItem {
                    field: field.to_string(),
                    problem,
                    context: if err.details.as_object().is_some_and(|o| !o.is_empty()) {
                        Some(err.details)
                    } else {
                        None
                    },
                });
                None
            }
        }
    }

    pub fn push(&mut self, field: &str, problem: &str, context: Option<serde_json::Value>) {
        self.errors.push(crate::error::ValidationErrorItem {
            field: field.to_string(),
            problem: problem.to_string(),
            context,
        });
    }

    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    pub fn finish(self) -> Result<()> {
        match self.errors.len() {
            0 => Ok(()),
            1 => {
                let err = &self.errors[0];
                Err(Error::validation_invalid_argument(
                    &err.field,
                    &err.problem,
                    None,
                    None,
                ))
            }
            _ => Err(Error::validation_multiple_errors(self.errors)),
        }
    }
}

impl Default for ValidationCollector {
    fn default() -> Self {
        Self::new()
    }
}
