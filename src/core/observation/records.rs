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
