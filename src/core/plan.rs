//! Generic typed plan substrate for Homeboy workflows.
//!
//! Domain-specific planners can keep their existing public JSON contracts while
//! adapting toward this shared shape. The common model answers what will run,
//! why it will run, whether it blocks progress, and what artifacts/results are
//! expected.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A typed plan that can describe quality, build, release, deploy, PR, CI,
/// refactor, review, or domain-specific workflows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HomeboyPlan {
    pub id: String,
    pub kind: PlanKind,
    pub subject: PlanSubject,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub inputs: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub policy: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<PlanStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<PlanArtifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<PlanSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

/// High-level plan family. `Custom` gives extensions and future command
/// families a stable escape hatch without changing the schema shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    Quality,
    Build,
    Release,
    Deploy,
    PullRequest,
    Ci,
    Refactor,
    Review,
    Custom,
}

/// The thing a plan is about: a component, a file/symbol scope, or a prose
/// description when no stronger subject type exists yet.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSubject {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One planned unit of work.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "default_blocking")]
    pub blocking: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    pub status: PlanStepStatus,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub inputs: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub outputs: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub policy: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Ready,
    Missing,
    Disabled,
    Skipped,
    Running,
    Success,
    PartialSuccess,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanArtifact {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_type: Option<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub data: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanSummary {
    pub total_steps: usize,
    pub ready: usize,
    pub blocked: usize,
    pub skipped: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_actions: Vec<String>,
}

fn default_blocking() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{HomeboyPlan, PlanKind, PlanStep, PlanStepStatus, PlanSubject};
    use std::collections::HashMap;

    #[test]
    fn serializes_plan_kind_as_snake_case() {
        let serialized = serde_json::to_value(PlanKind::PullRequest).expect("serialize plan kind");

        assert_eq!(serialized, serde_json::json!("pull_request"));
    }

    #[test]
    fn serializes_minimal_component_plan() {
        let plan = HomeboyPlan {
            id: "release.homeboy".to_string(),
            kind: PlanKind::Release,
            subject: PlanSubject {
                component_id: Some("homeboy".to_string()),
                ..PlanSubject::default()
            },
            mode: None,
            inputs: HashMap::new(),
            policy: HashMap::new(),
            steps: Vec::new(),
            artifacts: Vec::new(),
            summary: None,
            warnings: Vec::new(),
            hints: Vec::new(),
        };

        let serialized = serde_json::to_value(&plan).expect("serialize plan");

        assert_eq!(
            serialized,
            serde_json::json!({
                "id": "release.homeboy",
                "kind": "release",
                "subject": {
                    "component_id": "homeboy"
                }
            })
        );
    }

    #[test]
    fn deserializes_default_blocking_step() {
        let step: PlanStep = serde_json::from_value(serde_json::json!({
            "id": "lint",
            "kind": "quality.lint",
            "status": "ready"
        }))
        .expect("deserialize step");

        assert!(step.blocking);
        assert!(step.scope.is_empty());
        assert!(step.inputs.is_empty());
    }

    #[test]
    fn serializes_step_status_as_snake_case() {
        let serialized =
            serde_json::to_value(PlanStepStatus::PartialSuccess).expect("serialize step status");

        assert_eq!(serialized, serde_json::json!("partial_success"));
    }
}
