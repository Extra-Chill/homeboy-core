use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Pass,
    Fail,
    Error,
    Skipped,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Error => "error",
            Self::Skipped => "skipped",
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ArtifactRecord {
    pub id: String,
    pub run_id: String,
    pub kind: String,
    pub path: String,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub mime: Option<String>,
    pub created_at: String,
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

#[derive(Debug, Clone, Serialize, PartialEq)]
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
    }
}
