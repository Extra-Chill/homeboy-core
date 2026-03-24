//! grammar_definition_loaded — extracted from grammar.rs.

use super::default;


pub(crate) fn default_fallback_default() -> String {
    "Default::default()".to_string()
}

pub(crate) fn default_group_1() -> usize {
    1
}

pub(crate) fn default_group_2() -> usize {
    2
}

pub(crate) fn default_return_type_separator() -> String {
    "->".to_string()
}

pub(crate) fn default_param_format() -> String {
    "name_colon_type".to_string()
}

pub(crate) fn default_quotes() -> Vec<String> {
    vec!["\"".to_string(), "'".to_string()]
}

pub(crate) fn default_escape_string() -> String {
    "\\".to_string()
}

pub(crate) fn default_open() -> String {
    "{".to_string()
}

pub(crate) fn default_close() -> String {
    "}".to_string()
}

pub(crate) fn default_context() -> String {
    "any".to_string()
}

pub(crate) fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_fallback_default_default_path() {

        let _result = default_fallback_default();
    }

    #[test]
    fn test_default_group_1_default_path() {

        let _result = default_group_1();
    }

    #[test]
    fn test_default_group_2_default_path() {

        let _result = default_group_2();
    }

    #[test]
    fn test_default_return_type_separator_default_path() {

        let _result = default_return_type_separator();
    }

    #[test]
    fn test_default_param_format_default_path() {

        let _result = default_param_format();
    }

    #[test]
    fn test_default_quotes_default_path() {

        let result = default_quotes();
        assert!(!result.is_empty(), "expected non-empty collection for: default path");
    }

    #[test]
    fn test_default_escape_string_default_path() {

        let _result = default_escape_string();
    }

    #[test]
    fn test_default_open_default_path() {

        let _result = default_open();
    }

    #[test]
    fn test_default_close_default_path() {

        let _result = default_close();
    }

    #[test]
    fn test_default_context_default_path() {

        let _result = default_context();
    }

    #[test]
    fn test_default_true_default_path() {

        let _result = default_true();
    }

}
