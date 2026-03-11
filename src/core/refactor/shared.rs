use crate::code_audit::conventions::Language;
use std::path::Path;

pub(crate) fn detect_language(path: &Path) -> Language {
    path.extension()
        .and_then(|e| e.to_str())
        .map(Language::from_extension)
        .unwrap_or(Language::Unknown)
}
