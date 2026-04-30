//! Trace span baseline and ratchet support.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::baseline::{self as generic, BaselineConfig};
use crate::error::Result;

use super::parsing::TraceResults;

const BASELINE_KEY: &str = "trace";
pub const DEFAULT_REGRESSION_THRESHOLD_PERCENT: f64 = 5.0;

fn baseline_key_for(rig_id: Option<&str>) -> String {
    rig_id.map_or_else(|| BASELINE_KEY.to_string(), |id| format!("trace.rig.{id}"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceSpanSnapshot {
    pub id: String,
    pub duration_ms: u64,
}

impl generic::Fingerprintable for TraceSpanSnapshot {
    fn fingerprint(&self) -> String {
        format!("trace-span:{}", self.id)
    }

    fn description(&self) -> String {
        format!("{}ms", self.duration_ms)
    }

    fn context_label(&self) -> String {
        format!("trace span {}", self.id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceBaselineMetadata {
    pub spans: Vec<TraceSpanSnapshot>,
}

pub type TraceBaseline = generic::Baseline<TraceBaselineMetadata>;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TraceSpanDelta {
    pub id: String,
    pub baseline_duration_ms: u64,
    pub current_duration_ms: u64,
    pub delta_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_pct: Option<f64>,
    pub regression: bool,
    pub improvement: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TraceBaselineComparison {
    pub threshold_percent: f64,
    pub spans: Vec<TraceSpanDelta>,
    pub new_span_ids: Vec<String>,
    pub removed_span_ids: Vec<String>,
    pub regression: bool,
    pub has_improvements: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

pub fn save_baseline(
    source_path: &Path,
    component_id: &str,
    results: &TraceResults,
    rig_id: Option<&str>,
) -> Result<std::path::PathBuf> {
    let snapshots = snapshots_from_results(results);
    let metadata = TraceBaselineMetadata {
        spans: snapshots.clone(),
    };
    let key = baseline_key_for(rig_id);
    let config = BaselineConfig::new(source_path, key);
    generic::save(&config, component_id, &snapshots, metadata)
}

pub fn load_baseline(source_path: &Path, rig_id: Option<&str>) -> Option<TraceBaseline> {
    let key = baseline_key_for(rig_id);
    let config = BaselineConfig::new(source_path, key);
    generic::load::<TraceBaselineMetadata>(&config).unwrap_or_default()
}

pub fn compare(
    current: &TraceResults,
    baseline: &TraceBaseline,
    threshold_percent: f64,
) -> TraceBaselineComparison {
    let current_snapshots = snapshots_from_results(current);
    let baseline_by_id: HashMap<&str, &TraceSpanSnapshot> = baseline
        .metadata
        .spans
        .iter()
        .map(|span| (span.id.as_str(), span))
        .collect();
    let current_by_id: HashMap<&str, &TraceSpanSnapshot> = current_snapshots
        .iter()
        .map(|span| (span.id.as_str(), span))
        .collect();

    let mut spans = Vec::new();
    let mut new_span_ids = Vec::new();
    let mut reasons = Vec::new();
    let mut regression = false;
    let mut has_improvements = false;

    for current_span in &current_snapshots {
        let Some(prior) = baseline_by_id.get(current_span.id.as_str()) else {
            new_span_ids.push(current_span.id.clone());
            continue;
        };
        let delta_ms = current_span.duration_ms as i64 - prior.duration_ms as i64;
        let delta_pct = if prior.duration_ms == 0 {
            None
        } else {
            Some((delta_ms as f64 / prior.duration_ms as f64) * 100.0)
        };
        let span_regression = delta_pct.is_some_and(|pct| pct > threshold_percent);
        let span_improvement = delta_ms < 0;
        if span_regression {
            regression = true;
            reasons.push(format!(
                "{} regressed by {}ms ({:.2}%)",
                current_span.id,
                delta_ms,
                delta_pct.unwrap_or_default()
            ));
        }
        if span_improvement {
            has_improvements = true;
        }
        spans.push(TraceSpanDelta {
            id: current_span.id.clone(),
            baseline_duration_ms: prior.duration_ms,
            current_duration_ms: current_span.duration_ms,
            delta_ms,
            delta_pct,
            regression: span_regression,
            improvement: span_improvement,
        });
    }

    let removed_span_ids = baseline
        .metadata
        .spans
        .iter()
        .filter(|span| !current_by_id.contains_key(span.id.as_str()))
        .map(|span| span.id.clone())
        .collect();

    TraceBaselineComparison {
        threshold_percent,
        spans,
        new_span_ids,
        removed_span_ids,
        regression,
        has_improvements,
        reasons,
    }
}

fn snapshots_from_results(results: &TraceResults) -> Vec<TraceSpanSnapshot> {
    results
        .span_results
        .iter()
        .filter_map(|span| {
            span.duration_ms.map(|duration_ms| TraceSpanSnapshot {
                id: span.id.clone(),
                duration_ms,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::trace::parsing::{TraceSpanResult, TraceSpanStatus, TraceStatus};

    fn results(duration_ms: u64) -> TraceResults {
        TraceResults {
            component_id: "studio".to_string(),
            scenario_id: "create-site".to_string(),
            status: TraceStatus::Pass,
            summary: None,
            failure: None,
            rig: None,
            timeline: Vec::new(),
            span_definitions: Vec::new(),
            span_results: vec![TraceSpanResult {
                id: "submit_to_cli".to_string(),
                from: "ui.submit".to_string(),
                to: "cli.start".to_string(),
                status: TraceSpanStatus::Ok,
                duration_ms: Some(duration_ms),
                from_t_ms: Some(0),
                to_t_ms: Some(duration_ms),
                missing: Vec::new(),
                message: None,
            }],
            assertions: Vec::new(),
            artifacts: Vec::new(),
        }
    }

    #[test]
    fn test_compare() {
        let baseline = TraceBaseline {
            created_at: "2026-01-01T00:00:00Z".to_string(),
            context_id: "studio".to_string(),
            item_count: 1,
            known_fingerprints: vec!["submit_to_cli".to_string()],
            metadata: TraceBaselineMetadata {
                spans: vec![TraceSpanSnapshot {
                    id: "submit_to_cli".to_string(),
                    duration_ms: 100,
                }],
            },
        };

        let comparison = compare(&results(130), &baseline, 5.0);

        assert!(comparison.regression);
        assert_eq!(comparison.spans[0].delta_ms, 30);
        assert_eq!(comparison.spans[0].delta_pct, Some(30.0));
    }

    #[test]
    fn test_save_baseline() {
        let temp = tempfile::tempdir().unwrap();
        save_baseline(temp.path(), "studio", &results(100), None).unwrap();

        let loaded = load_baseline(temp.path(), None).expect("baseline loads");
        assert_eq!(loaded.metadata.spans[0].id, "submit_to_cli");
        assert_eq!(loaded.known_fingerprints[0], "trace-span:submit_to_cli");
    }

    #[test]
    fn test_load_baseline() {
        let temp = tempfile::tempdir().unwrap();

        assert!(load_baseline(temp.path(), Some("studio-rig")).is_none());
    }
}
