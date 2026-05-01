use std::path::{Path, PathBuf};

use homeboy::engine::run_dir::{self, RunDir};
use homeboy::extension::bench::report::collect_artifacts;
use homeboy::extension::bench::{BenchResults, BenchRunWorkflowResult};
use homeboy::git::short_head_revision_at;
use homeboy::observation::{NewRunRecord, ObservationStore, RunRecord, RunStatus};
use homeboy::rig::RigStateSnapshot;

use super::BenchRunArgs;

pub(super) struct BenchObservation {
    store: ObservationStore,
    run: RunRecord,
    initial_metadata: serde_json::Value,
}

pub(super) struct BenchObservationStart<'a> {
    pub component_id: &'a str,
    pub component_label: &'a str,
    pub source_path: &'a Path,
    pub args: &'a BenchRunArgs,
    pub selected_scenarios: &'a [String],
    pub rig_id: Option<&'a str>,
    pub rig_snapshot: Option<&'a RigStateSnapshot>,
    pub run_dir: &'a RunDir,
}

pub(super) fn start(start: BenchObservationStart<'_>) -> Option<BenchObservation> {
    let store = ObservationStore::open_initialized().ok()?;
    let metadata = bench_observation_initial_metadata(
        start.component_label,
        start.args,
        start.selected_scenarios,
        start.rig_snapshot,
        start.run_dir,
    );
    let run = store
        .start_run(NewRunRecord {
            kind: "bench".to_string(),
            component_id: Some(start.component_id.to_string()),
            command: Some(bench_observation_command(
                start.component_id,
                start.args,
                start.rig_id,
            )),
            cwd: Some(start.source_path.to_string_lossy().to_string()),
            homeboy_version: Some(env!("CARGO_PKG_VERSION").to_string()),
            git_sha: short_head_revision_at(start.source_path),
            rig_id: start.rig_id.map(str::to_string),
            metadata_json: metadata.clone(),
        })
        .ok()?;

    Some(BenchObservation {
        store,
        run,
        initial_metadata: metadata,
    })
}

pub(super) fn finish_success(
    observation: Option<BenchObservation>,
    workflow: &BenchRunWorkflowResult,
    run_dir: &RunDir,
) {
    let Some(observation) = observation else {
        return;
    };

    record_bench_observation_artifacts(&observation, workflow, run_dir);
    let metadata = bench_observation_finish_metadata(observation.initial_metadata, workflow);
    let status = if workflow.exit_code == 0 {
        RunStatus::Pass
    } else {
        RunStatus::Fail
    };
    let _ = observation
        .store
        .finish_run(&observation.run.id, status, Some(metadata));
}

pub(super) fn finish_error(
    observation: Option<BenchObservation>,
    error: &homeboy::Error,
    run_dir: &RunDir,
) {
    let Some(observation) = observation else {
        return;
    };

    record_if_exists(
        &observation,
        "bench_results",
        run_dir.step_file(run_dir::files::BENCH_RESULTS),
    );
    record_if_exists(
        &observation,
        "resource_summary",
        run_dir.step_file(run_dir::files::RESOURCE_SUMMARY),
    );
    let metadata = merge_observation_metadata(
        observation.initial_metadata,
        serde_json::json!({
            "observation_status": "error",
            "error": error.to_string(),
        }),
    );
    let _ = observation
        .store
        .finish_run(&observation.run.id, RunStatus::Error, Some(metadata));
}

fn bench_observation_command(
    component_id: &str,
    args: &BenchRunArgs,
    rig_id: Option<&str>,
) -> String {
    let mut parts = vec![
        "homeboy".to_string(),
        "bench".to_string(),
        component_id.to_string(),
    ];
    if let Some(rig_id) = rig_id {
        parts.push(format!("--rig={rig_id}"));
    }
    if args.iterations != 10 {
        parts.push(format!("--iterations={}", args.iterations));
    }
    if args.runs != 1 {
        parts.push(format!("--runs={}", args.runs));
    }
    if args.concurrency != 1 {
        parts.push(format!("--concurrency={}", args.concurrency));
    }
    parts.join(" ")
}

fn bench_observation_initial_metadata(
    component_label: &str,
    args: &BenchRunArgs,
    selected_scenarios: &[String],
    rig_snapshot: Option<&RigStateSnapshot>,
    run_dir: &RunDir,
) -> serde_json::Value {
    serde_json::json!({
        "component_label": component_label,
        "iterations": args.iterations,
        "warmup_iterations": args.warmup,
        "runs": args.runs,
        "concurrency": args.concurrency,
        "regression_threshold_percent": args.regression_threshold,
        "baseline": {
            "baseline": args.baseline_args.baseline,
            "ignore_baseline": args.baseline_args.ignore_baseline,
            "ratchet": args.baseline_args.ratchet,
        },
        "profile": args.profile,
        "selected_scenarios": selected_scenarios,
        "shared_state": args.shared_state.as_ref().map(|path| path.to_string_lossy().to_string()),
        "run_dir": run_dir.path().to_string_lossy().to_string(),
        "rig_state": rig_snapshot,
    })
}

fn bench_observation_finish_metadata(
    initial_metadata: serde_json::Value,
    workflow: &BenchRunWorkflowResult,
) -> serde_json::Value {
    merge_observation_metadata(
        initial_metadata,
        serde_json::json!({
            "observation_status": workflow.status,
            "exit_code": workflow.exit_code,
            "gate_failures": workflow.gate_failures,
            "baseline_status": baseline_status(workflow),
            "failure": workflow.failure,
            "results": workflow.results,
            "scenario_metrics": workflow.results.as_ref().map(scenario_metric_summaries).unwrap_or_default(),
        }),
    )
}

fn merge_observation_metadata(
    mut initial: serde_json::Value,
    finish: serde_json::Value,
) -> serde_json::Value {
    if let (Some(initial), Some(finish)) = (initial.as_object_mut(), finish.as_object()) {
        for (key, value) in finish {
            initial.insert(key.clone(), value.clone());
        }
    }
    initial
}

fn baseline_status(workflow: &BenchRunWorkflowResult) -> Option<&'static str> {
    workflow.baseline_comparison.as_ref().map(|comparison| {
        if comparison.regression {
            "regression"
        } else if comparison.has_improvements {
            "improved"
        } else {
            "unchanged"
        }
    })
}

fn scenario_metric_summaries(results: &BenchResults) -> Vec<serde_json::Value> {
    results
        .scenarios
        .iter()
        .map(|scenario| {
            serde_json::json!({
                "scenario_id": scenario.id,
                "iterations": scenario.iterations,
                "passed": scenario.passed,
                "metrics": scenario.metrics,
                "metric_groups": scenario.metric_groups,
                "memory": scenario.memory,
                "artifact_count": scenario.artifacts.len(),
                "run_count": scenario.runs.as_ref().map(Vec::len),
                "runs_summary": scenario.runs_summary,
            })
        })
        .collect()
}

fn record_bench_observation_artifacts(
    observation: &BenchObservation,
    workflow: &BenchRunWorkflowResult,
    run_dir: &RunDir,
) {
    record_if_exists(
        observation,
        "bench_results",
        run_dir.step_file(run_dir::files::BENCH_RESULTS),
    );
    record_if_exists(
        observation,
        "resource_summary",
        run_dir.step_file(run_dir::files::RESOURCE_SUMMARY),
    );

    let Some(results) = workflow.results.as_ref() else {
        return;
    };
    for artifact in collect_artifacts(results) {
        let path = resolve_bench_artifact_path(&artifact.path, run_dir);
        record_if_exists(observation, "bench_artifact", path);
    }
}

fn record_if_exists(observation: &BenchObservation, kind: &str, path: PathBuf) {
    if path.is_file() {
        let _ = observation
            .store
            .record_artifact(&observation.run.id, kind, path);
    }
}

fn resolve_bench_artifact_path(path: &str, run_dir: &RunDir) -> PathBuf {
    let artifact_path = PathBuf::from(path);
    if artifact_path.is_absolute() || artifact_path.exists() {
        return artifact_path;
    }
    let run_dir_path = run_dir.path().join(path);
    if run_dir_path.exists() {
        return run_dir_path;
    }
    artifact_path
}

#[cfg(test)]
mod tests {
    use std::fs;

    use homeboy::engine::run_dir::{self, RunDir};
    use homeboy::extension::bench::artifact::BenchArtifact;
    use homeboy::extension::bench::{BenchResults, BenchRunWorkflowResult};
    use homeboy::observation::ObservationStore;

    use super::*;
    use crate::commands::bench::BenchRigOrder;
    use crate::commands::utils::args::{
        BaselineArgs, ExtensionOverrideArgs, HiddenJsonArgs, PositionalComponentArgs, SettingArgs,
    };
    use crate::test_support::with_isolated_home;

    struct XdgGuard(Option<String>);

    impl XdgGuard {
        fn unset() -> Self {
            let prior = std::env::var("XDG_DATA_HOME").ok();
            std::env::remove_var("XDG_DATA_HOME");
            Self(prior)
        }
    }

    impl Drop for XdgGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(value) => std::env::set_var("XDG_DATA_HOME", value),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    fn bench_results(component_id: &str, scenario_id: &str, p95: f64) -> BenchResults {
        serde_json::from_value(serde_json::json!({
            "component_id": component_id,
            "iterations": 10,
            "scenarios": [
                {
                    "id": scenario_id,
                    "iterations": 10,
                    "metrics": { "p95_ms": p95 }
                }
            ],
            "metric_policies": {
                "p95_ms": { "direction": "lower_is_better" }
            }
        }))
        .expect("bench results")
    }

    fn bench_args() -> BenchRunArgs {
        BenchRunArgs {
            comp: PositionalComponentArgs {
                component: Some("homeboy".to_string()),
                path: None,
            },
            extension_override: ExtensionOverrideArgs::default(),
            iterations: 10,
            warmup: None,
            runs: 1,
            shared_state: None,
            concurrency: 1,
            baseline_args: BaselineArgs::default(),
            regression_threshold: 5.0,
            setting_args: SettingArgs::default(),
            args: Vec::new(),
            _json: HiddenJsonArgs::default(),
            json_summary: false,
            report: Vec::new(),
            rig: Vec::new(),
            rig_order: BenchRigOrder::Input,
            scenario_ids: Vec::new(),
            profile: None,
            ignore_default_baseline: false,
        }
    }

    #[test]
    fn bench_observation_persists_success_with_metrics_and_artifacts() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let run_dir = RunDir::create().expect("run dir");
            fs::write(run_dir.step_file(run_dir::files::BENCH_RESULTS), b"{}").expect("results");
            fs::write(run_dir.step_file(run_dir::files::RESOURCE_SUMMARY), b"{}")
                .expect("resources");
            let artifact_path = run_dir.path().join("bench-artifacts/cold/transcript.json");
            fs::create_dir_all(artifact_path.parent().expect("artifact parent")).expect("mkdir");
            fs::write(&artifact_path, b"{\"ok\":true}").expect("artifact");

            let mut results = bench_results("homeboy", "cold", 42.0);
            results.scenarios[0].artifacts.insert(
                "transcript".to_string(),
                BenchArtifact {
                    path: "bench-artifacts/cold/transcript.json".to_string(),
                    url: None,
                    kind: Some("json".to_string()),
                    label: Some("Transcript".to_string()),
                },
            );
            let workflow = BenchRunWorkflowResult {
                status: "passed".to_string(),
                component: "homeboy".to_string(),
                exit_code: 0,
                iterations: 10,
                results: Some(results),
                gate_failures: Vec::new(),
                baseline_comparison: None,
                hints: None,
                failure: None,
            };

            let args = bench_args();
            let selected_scenarios = vec!["cold".to_string()];
            let observation = start(BenchObservationStart {
                component_id: "homeboy",
                component_label: "homeboy",
                source_path: home.path(),
                args: &args,
                selected_scenarios: &selected_scenarios,
                rig_id: None,
                rig_snapshot: None,
                run_dir: &run_dir,
            })
            .expect("start observation");
            let run_id = observation.run.id.clone();
            finish_success(Some(observation), &workflow, &run_dir);

            let store = ObservationStore::open_initialized().expect("store");
            let run = store.get_run(&run_id).expect("read run").expect("run");
            assert_eq!(run.kind, "bench");
            assert_eq!(run.status, "pass");
            assert_eq!(run.component_id.as_deref(), Some("homeboy"));
            assert_eq!(run.metadata_json["selected_scenarios"][0], "cold");
            assert_eq!(
                run.metadata_json["scenario_metrics"][0]["scenario_id"],
                "cold"
            );
            assert_eq!(
                run.metadata_json["scenario_metrics"][0]["metrics"]["p95_ms"],
                42.0
            );

            let artifacts = store.list_artifacts(&run_id).expect("artifacts");
            let kinds: Vec<_> = artifacts
                .iter()
                .map(|artifact| artifact.kind.as_str())
                .collect();
            assert!(kinds.contains(&"bench_results"));
            assert!(kinds.contains(&"resource_summary"));
            assert!(kinds.contains(&"bench_artifact"));
        });
    }

    #[test]
    fn bench_observation_persists_workflow_error() {
        with_isolated_home(|home| {
            let _xdg = XdgGuard::unset();
            let run_dir = RunDir::create().expect("run dir");
            fs::write(run_dir.step_file(run_dir::files::BENCH_RESULTS), b"{}").expect("results");
            let mut args = bench_args();
            args.rig = vec!["studio".to_string()];

            let observation = start(BenchObservationStart {
                component_id: "homeboy",
                component_label: "homeboy",
                source_path: home.path(),
                args: &args,
                selected_scenarios: &[],
                rig_id: Some("studio"),
                rig_snapshot: None,
                run_dir: &run_dir,
            })
            .expect("start observation");
            let run_id = observation.run.id.clone();
            let error = homeboy::Error::validation_invalid_argument(
                "bench",
                "synthetic bench error",
                None,
                None,
            );
            finish_error(Some(observation), &error, &run_dir);

            let store = ObservationStore::open_initialized().expect("store");
            let run = store.get_run(&run_id).expect("read run").expect("run");
            assert_eq!(run.status, "error");
            assert_eq!(run.rig_id.as_deref(), Some("studio"));
            assert!(run.metadata_json["error"]
                .as_str()
                .expect("error string")
                .contains("synthetic bench error"));
            assert_eq!(store.list_artifacts(&run_id).expect("artifacts").len(), 1);
        });
    }
}
