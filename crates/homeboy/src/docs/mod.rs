use std::collections::HashMap;
use std::sync::OnceLock;

use homeboy_core::token;

include!(concat!(env!("OUT_DIR"), "/generated_docs.rs"));

fn docs_index() -> &'static HashMap<&'static str, &'static str> {
    static DOCS: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();

    DOCS.get_or_init(|| GENERATED_DOCS.iter().copied().collect())
}

#[derive(Debug, Clone)]
pub struct ResolvedDoc {
    pub topic_label: String,
    pub key: String,
    pub segments: Vec<String>,
    pub content: String,
}

pub fn resolve(topic: &[String]) -> ResolvedDoc {
    let (topic_label, key, segments) = normalize_topic(topic);
    let content = docs_index().get(key.as_str()).copied().unwrap_or_default();

    ResolvedDoc {
        topic_label,
        key,
        segments,
        content: content.to_string(),
    }
}

fn normalize_topic(topic: &[String]) -> (String, String, Vec<String>) {
    if topic.is_empty() {
        return (
            "index".to_string(),
            "index".to_string(),
            vec!["index".to_string()],
        );
    }

    let user_label = topic.join(" ");

    let mut segments: Vec<String> = Vec::new();
    for raw in topic {
        for part in raw.split('/') {
            let segment = token::normalize_doc_segment(part);
            if !segment.is_empty() {
                segments.push(segment);
            }
        }
    }

    if segments.is_empty() {
        return (
            "unknown".to_string(),
            "index".to_string(),
            vec!["index".to_string()],
        );
    }

    let key = segments.join("/");

    if user_label.is_empty() {
        return ("unknown".to_string(), key, segments);
    }

    (user_label, key, segments)
}

pub fn available_topics() -> Vec<String> {
    let mut keys: Vec<&'static str> = GENERATED_DOCS.iter().map(|(key, _)| *key).collect();
    keys.sort_unstable();

    keys.into_iter().map(|key| key.to_string()).collect()
}
