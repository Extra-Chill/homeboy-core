use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_project_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_changelog_next_section_label: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_changelog_next_section_aliases: Option<Vec<String>>,
}
