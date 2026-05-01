use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

use crate::extension::lint::LintFinding;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Pass,
    Fail,
    Error,
    Skipped,
    Stale,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Error => "error",
            Self::Skipped => "skipped",
            Self::Stale => "stale",
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NewRunRecord {
    pub kind: String,
    pub component_id: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub homeboy_version: Option<String>,
    pub git_sha: Option<String>,
    pub rig_id: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunRecord {
    pub id: String,
    pub kind: String,
    pub component_id: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub command: Option<String>,
    pub cwd: Option<String>,
    pub homeboy_version: Option<String>,
    pub git_sha: Option<String>,
    pub rig_id: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunListFilter {
    pub kind: Option<String>,
    pub component_id: Option<String>,
    pub status: Option<String>,
    pub rig_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    #[serde(rename = "type", default = "default_artifact_type")]
    pub artifact_type: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub mime: Option<String>,
    pub created_at: String,
}

fn default_artifact_type() -> String {
    "file".to_string()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NewFindingRecord {
    pub run_id: String,
    pub tool: String,
    pub rule: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub severity: Option<String>,
    pub fingerprint: Option<String>,
    pub message: String,
    pub fixable: Option<bool>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FindingRecord {
    pub id: String,
    pub run_id: String,
    pub tool: String,
    pub rule: Option<String>,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub severity: Option<String>,
    pub fingerprint: Option<String>,
    pub message: String,
    pub fixable: Option<bool>,
    pub metadata_json: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindingListFilter {
    pub run_id: Option<String>,
    pub tool: Option<String>,
    pub file: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnnotationFindingRecord {
    pub file: Option<String>,
    pub line: Option<i64>,
    pub message: String,
    pub source: Option<String>,
    pub severity: Option<String>,
    pub code: Option<String>,
    pub fixable: Option<bool>,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, Value>,
}

pub fn finding_record_from_lint(run_id: &str, finding: &LintFinding) -> NewFindingRecord {
    NewFindingRecord {
        run_id: run_id.to_string(),
        tool: finding.tool.clone().unwrap_or_else(|| "lint".to_string()),
        rule: lint_extra_string(finding, "rule").or_else(|| Some(finding.category.clone())),
        file: finding.file.clone(),
        line: lint_extra_i64(finding, "line"),
        severity: finding.severity.clone(),
        fingerprint: Some(finding.id.clone()),
        message: finding.message.clone(),
        fixable: lint_extra_bool(finding, "fixable"),
        metadata_json: serde_json::json!({
            "category": finding.category,
            "source_sidecar": "lint-findings",
            "raw": finding,
        }),
    }
}

pub fn finding_records_from_lint(run_id: &str, findings: &[LintFinding]) -> Vec<NewFindingRecord> {
    findings
        .iter()
        .map(|finding| finding_record_from_lint(run_id, finding))
        .collect()
}

pub fn finding_records_from_annotations_dir(
    run_id: &str,
    annotations_dir: &Path,
) -> crate::error::Result<Vec<NewFindingRecord>> {
    if !annotations_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = std::fs::read_dir(annotations_dir)
        .map_err(|e| {
            crate::Error::internal_io(
                format!(
                    "Failed to read annotations dir {}: {}",
                    annotations_dir.display(),
                    e
                ),
                Some("observation.findings.annotations".to_string()),
            )
        })?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| {
            crate::Error::internal_io(
                format!(
                    "Failed to list annotations dir {}: {}",
                    annotations_dir.display(),
                    e
                ),
                Some("observation.findings.annotations".to_string()),
            )
        })?;
    entries.sort_by_key(|entry| entry.path());

    let mut records = Vec::new();
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        records.extend(finding_records_from_annotation_file(run_id, &path)?);
    }
    Ok(records)
}

pub fn finding_records_from_annotation_file(
    run_id: &str,
    path: &Path,
) -> crate::error::Result<Vec<NewFindingRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        crate::Error::internal_io(
            format!("Failed to read annotations file {}: {}", path.display(), e),
            Some("observation.findings.annotations".to_string()),
        )
    })?;
    if content.trim().is_empty() {
        return Ok(Vec::new());
    }

    let annotations: Vec<AnnotationFindingRecord> =
        serde_json::from_str(&content).map_err(|e| {
            crate::Error::internal_io(
                format!("Malformed annotations JSON in {}: {}", path.display(), e),
                Some("observation.findings.annotations".to_string()),
            )
        })?;
    let source_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("annotations.json");

    Ok(annotations
        .iter()
        .map(|annotation| finding_record_from_annotation(run_id, annotation, source_file))
        .collect())
}

pub fn finding_record_from_annotation(
    run_id: &str,
    annotation: &AnnotationFindingRecord,
    source_file: &str,
) -> NewFindingRecord {
    let tool = annotation
        .source
        .clone()
        .unwrap_or_else(|| annotation_file_stem(source_file));
    NewFindingRecord {
        run_id: run_id.to_string(),
        tool,
        rule: annotation.code.clone(),
        file: annotation.file.clone(),
        line: annotation.line,
        severity: annotation.severity.clone(),
        fingerprint: annotation_fingerprint(annotation),
        message: annotation.message.clone(),
        fixable: annotation.fixable,
        metadata_json: serde_json::json!({
            "source_sidecar": "annotations",
            "annotation_file": source_file,
            "raw": annotation,
        }),
    }
}

fn annotation_file_stem(source_file: &str) -> String {
    Path::new(source_file)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("annotations")
        .to_string()
}

fn annotation_fingerprint(annotation: &AnnotationFindingRecord) -> Option<String> {
    if let Some(id) = annotation.extra.get("id").and_then(Value::as_str) {
        return Some(id.to_string());
    }
    Some(format!(
        "{}:{}:{}:{}:{}",
        annotation.file.as_deref().unwrap_or_default(),
        annotation.line.unwrap_or_default(),
        annotation.source.as_deref().unwrap_or_default(),
        annotation.code.as_deref().unwrap_or_default(),
        annotation.message
    ))
}

fn lint_extra_string(finding: &LintFinding, key: &str) -> Option<String> {
    finding.extra.get(key)?.as_str().map(str::to_string)
}

fn lint_extra_i64(finding: &LintFinding, key: &str) -> Option<i64> {
    finding.extra.get(key)?.as_i64()
}

fn lint_extra_bool(finding: &LintFinding, key: &str) -> Option<bool> {
    match finding.extra.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NewTraceRunRecord {
    pub run_id: String,
    pub component_id: String,
    pub rig_id: Option<String>,
    pub scenario_id: String,
    pub status: String,
    pub baseline_status: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TraceRunRecord {
    pub run_id: String,
    pub component_id: String,
    pub rig_id: Option<String>,
    pub scenario_id: String,
    pub status: String,
    pub baseline_status: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NewTraceSpanRecord {
    pub run_id: String,
    pub span_id: String,
    pub status: String,
    pub duration_ms: Option<f64>,
    pub from_event: Option<String>,
    pub to_event: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceSpanRecord {
    pub id: String,
    pub run_id: String,
    pub span_id: String,
    pub status: String,
    pub duration_ms: Option<f64>,
    pub from_event: Option<String>,
    pub to_event: Option<String>,
    pub metadata_json: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_status_as_str() {
        assert_eq!(RunStatus::Running.as_str(), "running");
        assert_eq!(RunStatus::Pass.as_str(), "pass");
        assert_eq!(RunStatus::Fail.as_str(), "fail");
        assert_eq!(RunStatus::Error.as_str(), "error");
        assert_eq!(RunStatus::Skipped.as_str(), "skipped");
        assert_eq!(RunStatus::Stale.as_str(), "stale");
    }

    #[test]
    fn maps_lint_sidecar_finding_to_common_record() {
        let finding = LintFinding {
            id: "src/lib.rs:10:lint/security".to_string(),
            message: "escape output".to_string(),
            category: "security".to_string(),
            tool: Some("phpcs".to_string()),
            file: Some("src/lib.rs".to_string()),
            severity: Some("error".to_string()),
            extra: BTreeMap::from([
                ("line".to_string(), serde_json::json!(10)),
                ("rule".to_string(), serde_json::json!("WordPress.Security")),
                ("fixable".to_string(), serde_json::json!(true)),
            ]),
        };

        let record = finding_record_from_lint("run-1", &finding);

        assert_eq!(record.run_id, "run-1");
        assert_eq!(record.tool, "phpcs");
        assert_eq!(record.rule.as_deref(), Some("WordPress.Security"));
        assert_eq!(record.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(record.line, Some(10));
        assert_eq!(record.severity.as_deref(), Some("error"));
        assert_eq!(
            record.fingerprint.as_deref(),
            Some("src/lib.rs:10:lint/security")
        );
        assert_eq!(record.fixable, Some(true));
        assert_eq!(record.metadata_json["category"], "security");
        assert_eq!(record.metadata_json["source_sidecar"], "lint-findings");
    }

    #[test]
    fn maps_annotation_sidecar_file_to_common_records() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("phpcs.json");
        std::fs::write(
            &path,
            serde_json::to_string(&serde_json::json!([
                {
                    "file": "src/lib.rs",
                    "line": 12,
                    "message": "escape output",
                    "source": "phpcs",
                    "severity": "warning",
                    "code": "WordPress.Security.EscapeOutput",
                    "fixable": true,
                    "github_level": "warning"
                }
            ]))
            .expect("json"),
        )
        .expect("write");

        let records = finding_records_from_annotation_file("run-1", &path).expect("records");

        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.run_id, "run-1");
        assert_eq!(record.tool, "phpcs");
        assert_eq!(
            record.rule.as_deref(),
            Some("WordPress.Security.EscapeOutput")
        );
        assert_eq!(record.file.as_deref(), Some("src/lib.rs"));
        assert_eq!(record.line, Some(12));
        assert_eq!(record.severity.as_deref(), Some("warning"));
        assert_eq!(record.message, "escape output");
        assert_eq!(record.fixable, Some(true));
        assert!(record
            .fingerprint
            .as_deref()
            .expect("fingerprint")
            .contains("WordPress.Security.EscapeOutput"));
        assert_eq!(record.metadata_json["source_sidecar"], "annotations");
        assert_eq!(record.metadata_json["annotation_file"], "phpcs.json");
        assert_eq!(record.metadata_json["raw"]["github_level"], "warning");
    }
}
