use serde::Serialize;

use crate::docs;

use super::CmdResult;

#[derive(Serialize)]
pub struct ChangelogOutput {
    pub topic_label: String,
    pub content: String,
}

pub fn run() -> CmdResult<ChangelogOutput> {
    let resolved = docs::resolve(&["changelog".to_string()]);

    if resolved.content.is_empty() {
        return Err(homeboy_core::Error::Other(
            "No changelog found (expected embedded docs topic 'changelog')".to_string(),
        ));
    }

    Ok((
        ChangelogOutput {
            topic_label: resolved.topic_label,
            content: resolved.content,
        },
        0,
    ))
}
