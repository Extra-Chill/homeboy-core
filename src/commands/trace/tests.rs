use std::collections::HashMap;
use std::fs;
use std::process::Command;

use crate::test_support::with_isolated_home;

use homeboy::component::ScopedExtensionConfig;
use homeboy::rig::ComponentSpec;

use super::*;

fn aggregate_samples(durations: &[u64]) -> Vec<TraceAggregateSpanSample> {
    durations
        .iter()
        .enumerate()
        .map(|(index, duration_ms)| TraceAggregateSpanSample {
            duration_ms: *duration_ms,
            run_index: index + 1,
            artifact_path: format!("/tmp/trace-run-{}.json", index + 1),
        })
        .collect()
}

#[test]
fn rig_component_path_and_trace_env_are_threaded() {
    let component_dir = tempfile::TempDir::new().expect("component dir");
    let mut components = HashMap::new();
    let mut extensions = HashMap::new();
    extensions.insert(
        "trace-extension".to_string(),
        ScopedExtensionConfig::default(),
    );
    components.insert(
        "studio".to_string(),
        ComponentSpec {
            path: component_dir.path().to_string_lossy().to_string(),
            remote_url: Some("https://github.com/Automattic/studio".to_string()),
            triage_remote_url: None,
            stack: None,
            branch: None,
            extensions: Some(extensions),
        },
    );
    let spec = RigSpec {
        id: "studio-rig".to_string(),
        components,
        ..serde_json::from_str(r#"{"id":"studio-rig"}"#).unwrap()
    };

    let path = rig_component_path(&spec, "studio").expect("path resolves");
    assert_eq!(path, component_dir.path().to_string_lossy());
    let component = rig_component_for_trace(&spec, "studio").expect("component resolves");
    assert_eq!(component.id, "studio");
    assert_eq!(component.local_path, path);
    assert!(component.extensions.is_some());
}

#[test]
fn rig_component_for_trace_synthesizes_trace_workload_extensions() {
    let rig_spec: RigSpec = serde_json::from_str(
        r#"{
                "id": "studio",
                "components": {
                    "studio": { "path": "/tmp/studio" }
                },
                "trace_workloads": {
                    "nodejs": ["/tmp/create-site.trace.mjs"]
                }
            }"#,
    )
    .expect("parse rig spec");

    let component = rig_component_for_trace(&rig_spec, "studio").expect("component");

    assert!(component
        .extensions
        .as_ref()
        .expect("extensions")
        .contains_key("nodejs"));
}

#[test]
fn rig_trace_list_uses_rig_default_component_and_workloads() {
    with_isolated_home(|home| {
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (output, exit_code) = run_list(TraceArgs {
            comp: PositionalComponentArgs {
                component: None,
                path: None,
            },
            scenario: Some("list".to_string()),
            compare_after: None,
            rig: Some("studio-rig".to_string()),
            setting_args: SettingArgs::default(),
            _json: HiddenJsonArgs::default(),
            json_summary: false,
            report: None,
            experiment: None,
            repeat: 1,
            aggregate: None,
            schedule: TraceSchedule::Grouped,
            focus_spans: Vec::new(),
            spans: Vec::new(),
            phases: Vec::new(),
            phase_preset: None,
            baseline_args: BaselineArgs::default(),
            regression_threshold: extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
            overlays: Vec::new(),
            variants: Vec::new(),
            output_dir: None,
            keep_overlay: false,
            stale: false,
            force: false,
        })
        .expect("rig trace list should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::List(result) => {
                assert_eq!(result.component, "studio");
                assert_eq!(result.component_id, "studio");
                assert_eq!(result.count, 2);
                assert_eq!(result.scenarios[0].id, "studio-app-create-site");
                let expected_source = format!(
                    "{}/studio-app-create-site.trace.mjs",
                    component_dir.path().display()
                );
                assert_eq!(
                    result.scenarios[0].source.as_deref(),
                    Some(expected_source.as_str())
                );
            }
            _ => panic!("expected list output"),
        }
    });
}

#[test]
fn rig_trace_list_uses_scoped_workload_preflight() {
    with_isolated_home(|home| {
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
        fs::create_dir_all(&rig_dir).expect("mkdir rigs");
        fs::write(
                rig_dir.join("studio-rig.json"),
                format!(
                    r#"{{
                        "components": {{
                            "studio": {{ "path": "{}" }}
                        }},
                        "pipeline": {{
                            "check": [
                                {{
                                    "kind": "check",
                                    "label": "desktop app packaged",
                                    "groups": ["desktop-app"],
                                    "command": "true"
                                }},
                                {{
                                    "kind": "check",
                                    "label": "unrelated cli symlink",
                                    "groups": ["cli-dev-copy"],
                                    "command": "false"
                                }}
                            ]
                        }},
                        "trace_workloads": {{ "nodejs": [
                            {{
                                "path": "${{components.studio.path}}/studio-app-create-site.trace.mjs",
                                "check_groups": ["desktop-app"]
                            }}
                        ] }}
                    }}"#,
                    component_dir.path().display()
                ),
            )
            .expect("write rig");

        let (output, exit_code) = run_list(TraceArgs {
            comp: PositionalComponentArgs {
                component: None,
                path: None,
            },
            scenario: Some("list".to_string()),
            compare_after: None,
            rig: Some("studio-rig".to_string()),
            setting_args: SettingArgs::default(),
            _json: HiddenJsonArgs::default(),
            json_summary: false,
            report: None,
            experiment: None,
            repeat: 1,
            aggregate: None,
            schedule: TraceSchedule::Grouped,
            focus_spans: Vec::new(),
            spans: Vec::new(),
            phases: Vec::new(),
            phase_preset: None,
            baseline_args: BaselineArgs::default(),
            regression_threshold: extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
            overlays: Vec::new(),
            variants: Vec::new(),
            output_dir: None,
            keep_overlay: false,
            stale: false,
            force: false,
        })
        .expect("scoped rig trace list should bypass unrelated failed check");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::List(result) => {
                assert_eq!(result.count, 1);
                assert_eq!(result.scenarios[0].id, "studio-app-create-site");
            }
            _ => panic!("expected list output"),
        }
    });
}

#[test]
fn rig_trace_run_uses_rig_owned_workload_extension_without_component_link() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 1,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("rig trace run should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Run(result) => {
                assert!(result.passed);
                assert_eq!(result.component, "studio");
                assert_eq!(
                    result.results.expect("results").scenario_id,
                    "studio-app-create-site"
                );
            }
            _ => panic!("expected run output"),
        }
    });
}

#[test]
fn trace_run_persists_observation_history() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (_output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 1,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("trace should run");

        assert_eq!(exit_code, 0);
        let store = ObservationStore::open_initialized().expect("store");
        let runs = store
            .list_runs(homeboy::observation::RunListFilter {
                kind: Some("trace".to_string()),
                ..Default::default()
            })
            .expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "pass");
        assert_eq!(runs[0].component_id.as_deref(), Some("studio"));
        assert_eq!(runs[0].rig_id.as_deref(), Some("studio-rig"));

        let trace_run = store
            .get_trace_run(&runs[0].id)
            .expect("trace run")
            .expect("trace run row");
        assert_eq!(trace_run.component_id, "studio");
        assert_eq!(trace_run.scenario_id, "studio-app-create-site");
        assert_eq!(trace_run.status, "pass");
        assert_eq!(trace_run.metadata_json["span_count"], 1);

        let spans = store.list_trace_spans(&runs[0].id).expect("spans");
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].span_id, "boot_to_ready");
        assert_eq!(spans[0].duration_ms, Some(125.0));

        let artifacts = store.list_artifacts(&runs[0].id).expect("artifacts");
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.kind == "trace-results"));
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.kind == "trace-artifact"));
    });
}

#[test]
fn trace_repeat_aggregates_span_timings_and_preserves_artifacts() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 3,
                aggregate: Some("spans".to_string()),
                schedule: TraceSchedule::Interleaved,
                focus_spans: vec!["boot_to_ready".to_string()],
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("repeat trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Aggregate(aggregate) => {
                assert_eq!(aggregate.repeat, 3);
                assert_eq!(aggregate.run_count, 3);
                assert_eq!(aggregate.failure_count, 0);
                assert_eq!(aggregate.schedule.as_deref(), Some("interleaved"));
                assert_eq!(aggregate.run_order.len(), 3);
                assert_eq!(aggregate.run_order[0].index, 1);
                assert_eq!(aggregate.run_order[0].group, "run");
                assert_eq!(aggregate.run_order[0].iteration, 1);
                assert_eq!(aggregate.spans.len(), 1);
                assert_eq!(aggregate.focus_span_ids, vec!["boot_to_ready"]);
                assert_eq!(aggregate.focus_spans.len(), 1);
                let span = &aggregate.spans[0];
                assert_eq!(span.id, "boot_to_ready");
                assert_eq!(span.n, 3);
                assert_eq!(span.min_ms, Some(125));
                assert_eq!(span.median_ms, Some(125));
                assert_eq!(span.avg_ms, Some(125.0));
                assert_eq!(span.p75_ms, None);
                assert_eq!(span.p90_ms, None);
                assert_eq!(span.p95_ms, None);
                assert_eq!(span.max_ms, Some(125));
                assert!(matches!(span.max_run_index, Some(1..=3)));
                assert!(span
                    .max_artifact_path
                    .as_ref()
                    .is_some_and(|path| std::path::Path::new(path).is_file()));
                assert_eq!(span.failures, 0);
                assert!(aggregate
                    .runs
                    .iter()
                    .all(|run| std::path::Path::new(&run.artifact_path).is_file()));
            }
            _ => panic!("expected aggregate output"),
        }
    });
}

#[test]
fn trace_run_order_planner_supports_grouped_and_interleaved_variants() {
    let grouped = plan_trace_run_order(2, TraceSchedule::Grouped, &["baseline", "variant"]);
    assert_eq!(
        grouped
            .iter()
            .map(|entry| (entry.index, entry.group.as_str(), entry.iteration))
            .collect::<Vec<_>>(),
        vec![
            (1, "baseline", 1),
            (2, "baseline", 2),
            (3, "variant", 1),
            (4, "variant", 2),
        ]
    );

    let interleaved = plan_trace_run_order(2, TraceSchedule::Interleaved, &["baseline", "variant"]);
    assert_eq!(
        interleaved
            .iter()
            .map(|entry| (entry.index, entry.group.as_str(), entry.iteration))
            .collect::<Vec<_>>(),
        vec![
            (1, "baseline", 1),
            (2, "variant", 1),
            (3, "baseline", 2),
            (4, "variant", 2),
        ]
    );
}

#[test]
fn trace_repeat_reports_overlay_touched_files_at_top_level() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        init_overlay_component(component_dir.path());
        let patch_path = component_dir.path().join("overlay.patch");
        fs::write(
            &patch_path,
            r#"diff --git a/scenario.txt b/scenario.txt
--- a/scenario.txt
+++ b/scenario.txt
@@ -1 +1 @@
-base
+overlay
"#,
        )
        .expect("write patch");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 2,
                aggregate: Some("spans".to_string()),
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: vec![patch_path.to_string_lossy().to_string()],
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("repeat trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Aggregate(aggregate) => {
                assert_eq!(aggregate.overlays.len(), 1);
                let component_path = component_dir.path().to_string_lossy();
                assert_eq!(
                    aggregate.overlays[0].component_path,
                    component_path.as_ref()
                );
                assert_eq!(aggregate.overlays[0].touched_files, vec!["scenario.txt"]);
                assert!(!aggregate.overlays[0].kept);
                let value = serde_json::to_value(&aggregate).expect("aggregate serializes");
                assert_eq!(
                    value["overlays"][0]["component_path"],
                    component_path.as_ref()
                );
                assert_eq!(value["overlays"][0]["touched_files"][0], "scenario.txt");
            }
            _ => panic!("expected aggregate output"),
        }
    });
}

#[test]
fn trace_run_resolves_named_variants_and_reports_unknown_names() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        init_overlay_component(component_dir.path());
        let package_dir = tempfile::TempDir::new().expect("package dir");
        write_trace_rig_with_variant(
            home,
            package_dir.path(),
            "studio-rig",
            "studio",
            component_dir.path(),
        );

        let valid_args = TraceArgs {
            comp: PositionalComponentArgs {
                component: Some("studio".to_string()),
                path: None,
            },
            scenario: "studio-app-create-site".to_string(),
            compare_after: None,
            rig: Some("studio-rig".to_string()),
            setting_args: SettingArgs::default(),
            _json: HiddenJsonArgs::default(),
            json_summary: false,
            report: None,
            experiment: None,
            repeat: 1,
            aggregate: None,
            schedule: TraceSchedule::Grouped,
            focus_spans: Vec::new(),
            spans: Vec::new(),
            phases: Vec::new(),
            phase_preset: None,
            baseline_args: BaselineArgs::default(),
            regression_threshold: extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
            regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
            overlays: Vec::new(),
            variants: vec!["fresh-install-mode".to_string()],
            output_dir: None,
            keep_overlay: false,
            stale: false,
            force: false,
        };

        let (output, exit_code) =
            run(valid_args.clone(), &GlobalArgs {}).expect("variant trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Run(result) => {
                assert_eq!(result.overlays.len(), 1);
                let overlay = &result.overlays[0];
                assert_eq!(overlay.variant.as_deref(), Some("fresh-install-mode"));
                assert_eq!(
                    overlay.path,
                    package_dir
                        .path()
                        .join("overlays/fresh-install-mode.patch")
                        .to_string_lossy()
                );
                assert_eq!(overlay.touched_files, vec!["scenario.txt"]);
                let value = serde_json::to_value(&result).expect("result serializes");
                assert_eq!(value["overlays"][0]["variant"], "fresh-install-mode");
                assert_eq!(
                    value["overlays"][0]["path"],
                    package_dir
                        .path()
                        .join("overlays/fresh-install-mode.patch")
                        .to_string_lossy()
                        .as_ref()
                );
            }
            _ => panic!("expected run output"),
        }
        assert_eq!(
            fs::read_to_string(component_dir.path().join("scenario.txt")).unwrap(),
            "base\n"
        );

        let mut invalid_args = valid_args;
        invalid_args.variants = vec!["missing".to_string()];
        let err = match run(invalid_args, &GlobalArgs {}) {
            Ok(_) => panic!("unknown variant should fail"),
            Err(err) => err,
        };

        assert!(err.message.contains("unknown trace variant 'missing'"));
        assert!(err
            .details
            .get("id")
            .and_then(|value| value.as_str())
            .expect("details id")
            .contains("fresh-install-mode"));
    });
}

#[test]
fn trace_compare_variant_writes_experiment_bundle() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        init_overlay_component(component_dir.path());
        let patch_path = component_dir.path().join("overlay.patch");
        fs::write(
            &patch_path,
            r#"diff --git a/scenario.txt b/scenario.txt
--- a/scenario.txt
+++ b/scenario.txt
@@ -1 +1 @@
-base
+overlay
"#,
        )
        .expect("write patch");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());
        let output_dir = tempfile::TempDir::new().expect("output dir");

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("compare-variant".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 2,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: vec![patch_path.to_string_lossy().to_string()],
                variants: Vec::new(),
                output_dir: Some(output_dir.path().to_path_buf()),
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("compare-variant should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Compare(compare) => {
                assert_eq!(compare.span_count, 1);
                assert!(compare.before_path.ends_with("baseline.json"));
                assert!(compare.after_path.ends_with("variant.json"));
            }
            _ => panic!("expected compare output"),
        }
        assert!(output_dir.path().join("baseline.json").is_file());
        assert!(output_dir.path().join("variant.json").is_file());
        assert!(output_dir.path().join("compare.json").is_file());
        let summary = fs::read_to_string(output_dir.path().join("summary.md")).expect("summary");
        assert!(summary.contains("## Baseline Component SHAs"));
        assert!(summary.contains("## Variant Component SHAs"));
        assert!(summary.contains("scenario.txt"));
    });
}

#[test]
fn trace_compare_reports_median_and_average_deltas() {
    let before = TraceAggregateInput {
        component: Some("studio".to_string()),
        scenario_id: Some("create-site".to_string()),
        phase_preset: None,
        repeat: None,
        rig_state: None,
        overlays: Vec::new(),
        runs: Vec::new(),
        spans: vec![
            TraceAggregateSpanInput {
                id: "boot_to_ready".to_string(),
                n: 5,
                median_ms: Some(100),
                avg_ms: Some(110.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "large_improvement".to_string(),
                n: 5,
                median_ms: Some(300),
                avg_ms: Some(300.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "large_regression".to_string(),
                n: 5,
                median_ms: Some(80),
                avg_ms: Some(80.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "before_only".to_string(),
                n: 5,
                median_ms: Some(25),
                avg_ms: Some(25.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 1,
            },
        ],
    };
    let after = TraceAggregateInput {
        component: Some("studio".to_string()),
        scenario_id: Some("create-site".to_string()),
        phase_preset: None,
        repeat: None,
        rig_state: None,
        overlays: Vec::new(),
        runs: Vec::new(),
        spans: vec![
            TraceAggregateSpanInput {
                id: "boot_to_ready".to_string(),
                n: 5,
                median_ms: Some(125),
                avg_ms: Some(121.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "large_improvement".to_string(),
                n: 5,
                median_ms: Some(100),
                avg_ms: Some(100.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "large_regression".to_string(),
                n: 5,
                median_ms: Some(200),
                avg_ms: Some(200.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "after_only".to_string(),
                n: 3,
                median_ms: Some(75),
                avg_ms: Some(80.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
        ],
    };

    let compare = compare_trace_aggregates(
        Path::new("before.json"),
        before,
        Path::new("after.json"),
        after,
    );

    assert_eq!(compare.command, "trace.compare.spans");
    assert_eq!(compare.span_count, 5);
    assert_eq!(compare.spans[0].id, "large_improvement");
    assert_eq!(compare.spans[1].id, "large_regression");
    assert_eq!(compare.spans[2].id, "boot_to_ready");
    let changed = compare
        .spans
        .iter()
        .find(|span| span.id == "boot_to_ready")
        .expect("changed span");
    assert_eq!(changed.before_median_ms, Some(100));
    assert_eq!(changed.after_median_ms, Some(125));
    assert_eq!(changed.median_delta_ms, Some(25));
    assert_eq!(changed.median_delta_percent, Some(25.0));
    assert_eq!(changed.avg_delta_ms, Some(11.0));
    assert_eq!(changed.avg_delta_percent, Some(10.0));

    let before_only = compare
        .spans
        .iter()
        .find(|span| span.id == "before_only")
        .expect("before-only span");
    assert_eq!(before_only.after_n, None);
    assert_eq!(before_only.median_delta_ms, None);
}

#[test]
fn trace_compare_focus_spans_report_independent_regression_status() {
    let before = TraceAggregateInput {
        component: Some("studio".to_string()),
        scenario_id: Some("create-site".to_string()),
        phase_preset: None,
        repeat: None,
        rig_state: None,
        overlays: Vec::new(),
        runs: Vec::new(),
        spans: vec![
            TraceAggregateSpanInput {
                id: "focused".to_string(),
                n: 6,
                median_ms: Some(100),
                avg_ms: Some(100.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "unfocused".to_string(),
                n: 6,
                median_ms: Some(100),
                avg_ms: Some(100.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
        ],
    };
    let after = TraceAggregateInput {
        component: Some("studio".to_string()),
        scenario_id: Some("create-site".to_string()),
        phase_preset: None,
        repeat: None,
        rig_state: None,
        overlays: Vec::new(),
        runs: Vec::new(),
        spans: vec![
            TraceAggregateSpanInput {
                id: "focused".to_string(),
                n: 6,
                median_ms: Some(130),
                avg_ms: Some(130.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
            TraceAggregateSpanInput {
                id: "unfocused".to_string(),
                n: 6,
                median_ms: Some(250),
                avg_ms: Some(250.0),
                max_ms: None,
                max_run_index: None,
                max_artifact_path: None,
                failures: 0,
            },
        ],
    };

    let compare = compare_trace_aggregates_with_focus(
        Path::new("before.json"),
        before,
        Path::new("after.json"),
        after,
        &["focused".to_string()],
        20.0,
        10,
    );

    assert_eq!(compare.span_count, 2);
    assert_eq!(compare.spans.len(), 2);
    assert_eq!(compare.focus_span_ids, vec!["focused"]);
    assert_eq!(compare.focus_spans.len(), 1);
    assert_eq!(compare.focus_spans[0].id, "focused");
    assert_eq!(compare.focus_regression_count, 1);
    assert_eq!(compare.focus_failure_count, 0);
    assert_eq!(compare.focus_status.as_deref(), Some("fail"));
}

#[test]
fn trace_compare_accepts_json_summary_envelope_outputs() {
    let input = parse_trace_aggregate_input(
        r#"{
                "success": true,
                "data": {
                    "command": "trace.aggregate.spans",
                    "component": "studio",
                    "scenario_id": "create-site",
                    "spans": [
                        {
                            "id": "submit_to_running",
                            "n": 5,
                            "median_ms": 6059,
                            "avg_ms": 6019.8,
                            "failures": 0
                        }
                    ]
                }
            }"#,
    )
    .expect("json summary envelope should parse");

    assert_eq!(input.component.as_deref(), Some("studio"));
    assert_eq!(input.scenario_id.as_deref(), Some("create-site"));
    assert_eq!(input.spans.len(), 1);
    assert_eq!(input.spans[0].id, "submit_to_running");
    assert_eq!(input.spans[0].median_ms, Some(6059));
}

#[test]
fn trace_compare_markdown_and_experiment_bundle_render_artifacts() {
    let compare = extension_trace::TraceCompareOutput {
        command: "trace.compare.spans",
        before_path: "before.json".to_string(),
        after_path: "after.json".to_string(),
        before_component: Some("studio".to_string()),
        after_component: Some("studio".to_string()),
        before_scenario_id: Some("create-site".to_string()),
        after_scenario_id: Some("create-site".to_string()),
        span_count: 1,
        spans: vec![extension_trace::TraceCompareSpanOutput {
            id: "boot_to_ready".to_string(),
            before_n: Some(5),
            after_n: Some(5),
            before_median_ms: Some(100),
            after_median_ms: Some(125),
            median_delta_ms: Some(25),
            median_delta_percent: Some(25.0),
            before_avg_ms: Some(110.0),
            after_avg_ms: Some(121.0),
            avg_delta_ms: Some(11.0),
            avg_delta_percent: Some(10.0),
            before_failures: Some(0),
            after_failures: Some(0),
        }],
        focus_span_ids: Vec::new(),
        focus_spans: Vec::new(),
        focus_regression_count: 0,
        focus_failure_count: 0,
        focus_status: None,
    };

    let markdown = render_compare_markdown(&compare);

    assert!(markdown.contains("# Trace Compare"));
    assert!(markdown.contains("| Span | before median | after median | median delta | median % | before avg | after avg | avg delta | avg % |"));
    assert!(markdown.contains(
        "| `boot_to_ready` | 100ms | 125ms | **+25ms** | +25.0% | 110.0ms | 121.0ms | **+11.0ms** | +10.0% |"
    ));

    let dir = tempfile::TempDir::new().expect("bundle dir");
    let before_path = dir.path().join("baseline-source.json");
    let after_path = dir.path().join("variant-source.json");
    let overlay_path = dir.path().join("fast-install.patch");
    fs::write(&overlay_path, "diff --git a/install.ts b/install.ts\n").expect("write overlay");

    let before_json = serde_json::json!({
        "command": "trace.aggregate.spans",
        "component": "studio",
        "scenario_id": "studio-fast-install",
        "phase_preset": "startup",
        "repeat": 3,
        "rig_state": {
            "rig_id": "studio-rig",
            "captured_at": "2026-05-02T00:00:00Z",
            "components": {
                "studio": { "path": "/repo/studio", "branch": "main", "sha": "abc123" }
            }
        },
        "runs": [
            { "index": 1, "passed": true, "status": "pass", "exit_code": 0, "artifact_path": "/tmp/baseline-1.json" }
        ],
        "spans": [
            { "id": "install", "n": 3, "median_ms": 120, "avg_ms": 130.0, "max_ms": 160, "max_run_index": 1, "max_artifact_path": "/tmp/baseline-1.json", "failures": 0 }
        ]
    })
    .to_string();
    let after_json = serde_json::json!({
        "command": "trace.aggregate.spans",
        "component": "studio",
        "scenario_id": "studio-fast-install",
        "phase_preset": "startup",
        "repeat": 3,
        "rig_state": {
            "rig_id": "studio-rig",
            "captured_at": "2026-05-02T00:00:00Z",
            "components": {
                "studio": { "path": "/repo/studio", "branch": "trace-experiment-bundles", "sha": "def456" }
            }
        },
        "overlays": [
            { "path": overlay_path, "component_path": "/repo/studio", "touched_files": ["install.ts"], "kept": false }
        ],
        "runs": [
            { "index": 1, "passed": false, "status": "fail", "exit_code": 1, "artifact_path": "/tmp/variant-1.json", "failure": "assertion failed" }
        ],
        "spans": [
            { "id": "install", "n": 2, "median_ms": 80, "avg_ms": 90.0, "max_ms": 140, "max_run_index": 1, "max_artifact_path": "/tmp/variant-1.json", "failures": 1 }
        ]
    })
    .to_string();
    fs::write(&before_path, &before_json).expect("write before");
    fs::write(&after_path, &after_json).expect("write after");

    let before_for_compare = parse_trace_aggregate_input(&before_json).expect("before compare");
    let after_for_compare = parse_trace_aggregate_input(&after_json).expect("after compare");
    let compare = compare_trace_aggregates(
        &before_path,
        before_for_compare,
        &after_path,
        after_for_compare,
    );
    let before = parse_trace_aggregate_input(&before_json).expect("before bundle");
    let after = parse_trace_aggregate_input(&after_json).expect("after bundle");

    let bundle_dir = write_trace_experiment_bundle(TraceExperimentBundleRequest {
        name: "studio-fast-install",
        bundle_root: Some(dir.path()),
        command: "homeboy trace compare baseline-source.json variant-source.json --experiment studio-fast-install".to_string(),
        before_path: &before_path,
        before_json: &before_json,
        before: &before,
        after_path: &after_path,
        after_json: &after_json,
        after: &after,
        compare: &compare,
    })
    .expect("write bundle");

    assert!(bundle_dir.join("baseline.json").is_file());
    assert!(bundle_dir
        .join("variant-studio-fast-install.json")
        .is_file());
    assert!(bundle_dir
        .join("compare-studio-fast-install.json")
        .is_file());
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(bundle_dir.join("manifest.json")).expect("read manifest"),
    )
    .expect("parse manifest");
    assert!(manifest["command"]
        .as_str()
        .unwrap()
        .contains("trace compare"));
    assert_eq!(manifest["variants"][0]["role"], "baseline");
    assert_eq!(manifest["variants"][0]["phase_preset"], "startup");
    assert_eq!(manifest["variants"][0]["repeat"], 3);
    assert_eq!(manifest["variants"][0]["rig_id"], "studio-rig");
    assert_eq!(manifest["variants"][0]["components"][0]["sha"], "abc123");
    assert_eq!(
        manifest["variants"][1]["artifact_paths"][0],
        "/tmp/variant-1.json"
    );
    assert_eq!(manifest["overlays"][0]["touched_files"][0], "install.ts");
    assert_eq!(
        manifest["overlays"][0]["sha256"].as_str().unwrap().len(),
        64
    );
    assert!(Path::new(manifest["overlays"][0]["bundle_path"].as_str().unwrap()).is_file());

    let report = fs::read_to_string(bundle_dir.join("report.md")).expect("read report");
    assert!(report.contains("## Top Median Improvements"));
    assert!(report.contains("## Top Average Improvements"));
    assert!(report.contains("## Variant Failures and Outliers"));
    assert!(report.contains("/tmp/variant-1.json"));
}

#[test]
fn trace_run_expands_phase_chain_into_adjacent_and_total_spans() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 1,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: vec![
                    extension_trace::spans::TracePhaseMilestone {
                        label: "boot".to_string(),
                        key: "runner.boot".to_string(),
                    },
                    extension_trace::spans::TracePhaseMilestone {
                        label: "ready".to_string(),
                        key: "runner.ready".to_string(),
                    },
                ],
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("phase trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Run(result) => {
                let results = result.results.expect("results");
                let span_ids = results
                    .span_results
                    .iter()
                    .map(|span| (span.id.as_str(), span.duration_ms))
                    .collect::<Vec<_>>();
                assert_eq!(
                    span_ids,
                    vec![
                        ("phase.boot_to_ready", Some(125)),
                        ("phase.total", Some(125))
                    ]
                );
            }
            _ => panic!("expected run output"),
        }
    });
}

#[test]
fn trace_run_expands_named_workload_phase_preset() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig_with_phase_preset(home, "preset-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("preset-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 1,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: Some("startup".to_string()),
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("preset trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Run(result) => {
                let results = result.results.expect("results");
                let span_ids = results
                    .span_results
                    .iter()
                    .map(|span| (span.id.as_str(), span.duration_ms))
                    .collect::<Vec<_>>();
                assert_eq!(
                    span_ids,
                    vec![
                        ("phase.boot_to_ready", Some(125)),
                        ("phase.total", Some(125))
                    ]
                );
            }
            _ => panic!("expected run output"),
        }
    });
}

#[test]
fn trace_aggregate_spans_uses_workload_default_phase_preset() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig_with_phase_preset(home, "preset-rig", "studio", component_dir.path());

        let (output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("studio-app-create-site".to_string()),
                compare_after: None,
                rig: Some("preset-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 2,
                aggregate: Some("spans".to_string()),
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("aggregate trace should run");

        assert_eq!(exit_code, 0);
        match output {
            TraceCommandOutput::Aggregate(aggregate) => {
                let span_ids = aggregate
                    .spans
                    .iter()
                    .map(|span| span.id.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(span_ids, vec!["phase.boot_to_ready", "phase.total"]);
            }
            _ => panic!("expected aggregate output"),
        }
    });
}

#[test]
fn aggregate_span_reports_percentiles_when_sample_size_is_sufficient() {
    let span = aggregate_span(
        "boot_to_ready".to_string(),
        aggregate_samples(&[
            200, 10, 190, 20, 180, 30, 170, 40, 160, 50, 150, 60, 140, 70, 130, 80, 120, 90, 110,
            100,
        ]),
        2,
    );

    assert_eq!(span.n, 20);
    assert_eq!(span.min_ms, Some(10));
    assert_eq!(span.median_ms, Some(105));
    assert_eq!(span.avg_ms, Some(105.0));
    assert_eq!(span.p75_ms, Some(150));
    assert_eq!(span.p90_ms, Some(180));
    assert_eq!(span.p95_ms, Some(190));
    assert_eq!(span.max_ms, Some(200));
    assert_eq!(span.max_run_index, Some(1));
    assert_eq!(
        span.max_artifact_path.as_deref(),
        Some("/tmp/trace-run-1.json")
    );
    assert_eq!(span.failures, 2);
}

#[test]
fn aggregate_span_reports_run_and_artifact_for_max_sample() {
    let span = aggregate_span(
        "submit_to_running".to_string(),
        aggregate_samples(&[340, 11_757, 410]),
        0,
    );

    assert_eq!(span.max_ms, Some(11_757));
    assert_eq!(span.max_run_index, Some(2));
    assert_eq!(
        span.max_artifact_path.as_deref(),
        Some("/tmp/trace-run-2.json")
    );
}

#[test]
fn aggregate_span_omits_percentiles_for_small_sample_sizes() {
    let single = aggregate_span("single".to_string(), aggregate_samples(&[42]), 0);
    assert_eq!(single.min_ms, Some(42));
    assert_eq!(single.median_ms, Some(42));
    assert_eq!(single.avg_ms, Some(42.0));
    assert_eq!(single.p75_ms, None);
    assert_eq!(single.p90_ms, None);
    assert_eq!(single.p95_ms, None);
    assert_eq!(single.max_ms, Some(42));

    let four_samples = aggregate_span("four".to_string(), aggregate_samples(&[10, 20, 30, 40]), 0);
    assert_eq!(four_samples.p75_ms, Some(30));
    assert_eq!(four_samples.p90_ms, None);
    assert_eq!(four_samples.p95_ms, None);
}

#[test]
fn aggregate_markdown_includes_percentile_columns() {
    let aggregate = extension_trace::TraceAggregateOutput {
        command: "trace.aggregate.spans",
        passed: true,
        status: "pass".to_string(),
        component: "studio".to_string(),
        scenario_id: "create-site".to_string(),
        phase_preset: None,
        repeat: 20,
        run_count: 20,
        failure_count: 0,
        exit_code: 0,
        rig_state: None,
        schedule: None,
        run_order: Vec::new(),
        overlays: Vec::new(),
        runs: Vec::new(),
        spans: vec![aggregate_span(
            "boot_to_ready".to_string(),
            aggregate_samples(&((1..=20).map(|value| value * 10).collect::<Vec<_>>())),
            0,
        )],
        focus_span_ids: Vec::new(),
        focus_spans: Vec::new(),
    };

    let markdown = render_aggregate_markdown(&aggregate);

    assert!(
        markdown.contains("| Span | n | min | median | avg | p75 | p90 | p95 | max | failures |")
    );
    assert!(markdown.contains(
        "| `boot_to_ready` | 20 | 10ms | 105ms | 105.0ms | 150ms | 180ms | 190ms | 200ms | 0 |"
    ));
    assert!(markdown
        .contains("- `boot_to_ready`: run 20, max=200ms, artifact=`/tmp/trace-run-20.json`"));
}

#[test]
fn failed_trace_run_persists_observation_history() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::without_xdg_data_home();
        write_trace_extension(home);
        let component_dir = tempfile::TempDir::new().expect("component dir");
        write_trace_rig(home, "studio-rig", "studio", component_dir.path());

        let (_output, exit_code) = run(
            TraceArgs {
                comp: PositionalComponentArgs {
                    component: Some("studio".to_string()),
                    path: None,
                },
                scenario: Some("missing-scenario".to_string()),
                compare_after: None,
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                report: None,
                experiment: None,
                repeat: 1,
                aggregate: None,
                schedule: TraceSchedule::Grouped,
                focus_spans: Vec::new(),
                spans: Vec::new(),
                phases: Vec::new(),
                phase_preset: None,
                baseline_args: BaselineArgs::default(),
                regression_threshold:
                    extension_trace::baseline::DEFAULT_REGRESSION_THRESHOLD_PERCENT,
                regression_min_delta_ms: extension_trace::baseline::DEFAULT_REGRESSION_MIN_DELTA_MS,
                overlays: Vec::new(),
                variants: Vec::new(),
                output_dir: None,
                keep_overlay: false,
                stale: false,
                force: false,
            },
            &GlobalArgs {},
        )
        .expect("trace command should return structured failure output");

        assert_eq!(exit_code, 3);
        let store = ObservationStore::open_initialized().expect("store");
        let runs = store
            .list_runs(homeboy::observation::RunListFilter {
                kind: Some("trace".to_string()),
                ..Default::default()
            })
            .expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "error");

        let trace_run = store
            .get_trace_run(&runs[0].id)
            .expect("trace run")
            .expect("trace run row");
        assert_eq!(trace_run.status, "error");
        assert!(trace_run.metadata_json["failure"]["stderr_excerpt"]
            .as_str()
            .expect("stderr excerpt")
            .contains("unknown scenario missing-scenario"));
    });
}

struct XdgGuard(Option<String>);

impl XdgGuard {
    fn without_xdg_data_home() -> Self {
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

fn write_trace_extension(home: &tempfile::TempDir) {
    let extension_dir = home
        .path()
        .join(".config")
        .join("homeboy")
        .join("extensions")
        .join("nodejs");
    fs::create_dir_all(&extension_dir).expect("mkdir extension");
    fs::write(
        extension_dir.join("nodejs.json"),
        r#"{
                "name": "Node.js",
                "version": "0.0.0",
                "trace": { "extension_script": "trace-runner.sh" }
            }"#,
    )
    .expect("write extension manifest");

    let script_path = extension_dir.join("trace-runner.sh");
    fs::write(
            &script_path,
            r#"#!/bin/sh
set -eu
scenario_ids=""
old_ifs="$IFS"
IFS=":"
for workload in ${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}; do
  name="$(basename "$workload")"
  name="${name%%.trace.*}"
  name="${name%.*}"
  if [ -n "$scenario_ids" ]; then
    scenario_ids="$scenario_ids $name"
  else
    scenario_ids="$name"
  fi
done
IFS="$old_ifs"

if [ "$HOMEBOY_TRACE_LIST_ONLY" = "1" ]; then
  cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenarios":[
JSON
  comma=""
  printf '%s\n' "${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}" | tr ':' '\n' | while IFS= read -r workload; do
    [ -n "$workload" ] || continue
    name="$(basename "$workload")"
    name="${name%%.trace.*}"
    name="${name%.*}"
    printf '%s{"id":"%s","source":"%s"}' "$comma" "$name" "$workload" >> "$HOMEBOY_TRACE_RESULTS_FILE"
    comma=","
  done
  printf ']}' >> "$HOMEBOY_TRACE_RESULTS_FILE"
  exit 0
fi

case " $scenario_ids " in
  *" $HOMEBOY_TRACE_SCENARIO "*) ;;
  *) printf 'unknown scenario %s\n' "$HOMEBOY_TRACE_SCENARIO" >&2; exit 3 ;;
esac

cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenario_id":"$HOMEBOY_TRACE_SCENARIO","status":"pass","timeline":[{"t_ms":0,"source":"runner","event":"boot"},{"t_ms":125,"source":"runner","event":"ready"}],"span_results":[{"id":"boot_to_ready","from":"runner.boot","to":"runner.ready","status":"ok","duration_ms":125,"from_t_ms":0,"to_t_ms":125}],"assertions":[],"artifacts":[{"label":"trace log","path":"artifacts/trace-log.txt"}]}
JSON
mkdir -p "$HOMEBOY_TRACE_ARTIFACT_DIR"
printf 'trace log\n' > "$HOMEBOY_TRACE_ARTIFACT_DIR/trace-log.txt"
"#,
        )
        .expect("write trace script");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script_path)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script_path, permissions).expect("chmod script");
    }
}

fn init_overlay_component(path: &std::path::Path) {
    fs::write(path.join("scenario.txt"), "base\n").expect("write scenario");
    run_git(path, &["init"]);
    run_git(path, &["add", "scenario.txt"]);
    run_git(
        path,
        &[
            "-c",
            "user.name=Homeboy Test",
            "-c",
            "user.email=homeboy@example.test",
            "commit",
            "-m",
            "initial",
        ],
    );
}

fn run_git(path: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write_trace_rig(
    home: &tempfile::TempDir,
    rig_id: &str,
    component_id: &str,
    path: &std::path::Path,
) {
    let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
    fs::create_dir_all(&rig_dir).expect("mkdir rigs");
    fs::write(
        rig_dir.join(format!("{}.json", rig_id)),
        format!(
            r#"{{
                    "components": {{
                        "{component_id}": {{ "path": "{}" }}
                    }},
                    "trace_workloads": {{ "nodejs": [
                        "${{components.{component_id}.path}}/studio-app-create-site.trace.mjs",
                        "${{components.{component_id}.path}}/studio-list-sites.trace.mjs"
                    ] }}
                }}"#,
            path.display()
        ),
    )
    .expect("write rig");
}

fn write_trace_rig_with_phase_preset(
    home: &tempfile::TempDir,
    rig_id: &str,
    component_id: &str,
    path: &std::path::Path,
) {
    let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
    fs::create_dir_all(&rig_dir).expect("mkdir rigs");
    fs::write(
        rig_dir.join(format!("{}.json", rig_id)),
        format!(
            r#"{{
                    "components": {{
                        "{component_id}": {{ "path": "{}" }}
                    }},
                    "trace_workloads": {{ "nodejs": [
                        {{
                            "path": "${{components.{component_id}.path}}/studio-app-create-site.trace.mjs",
                            "check_groups": [],
                            "trace_default_phase_preset": "startup",
                            "trace_phase_presets": {{
                                "startup": ["boot:runner.boot", "ready:runner.ready"]
                            }}
                        }}
                    ] }}
                }}"#,
            path.display()
        ),
    )
    .expect("write rig");
}

fn write_trace_rig_with_variant(
    home: &tempfile::TempDir,
    package_path: &std::path::Path,
    rig_id: &str,
    component_id: &str,
    path: &std::path::Path,
) {
    let sources_dir = home
        .path()
        .join(".config")
        .join("homeboy")
        .join("rig-sources");
    fs::create_dir_all(&sources_dir).expect("mkdir rig sources");
    fs::write(
        sources_dir.join(format!("{}.json", rig_id)),
        format!(
            r#"{{
                "source": "{}",
                "package_path": "{}",
                "rig_path": "{}/rig.json",
                "linked": true,
                "source_revision": null
            }}"#,
            package_path.display(),
            package_path.display(),
            package_path.display()
        ),
    )
    .expect("write rig source metadata");

    let overlay_dir = package_path.join("overlays");
    fs::create_dir_all(&overlay_dir).expect("mkdir overlays");
    fs::write(
        overlay_dir.join("fresh-install-mode.patch"),
        r#"diff --git a/scenario.txt b/scenario.txt
--- a/scenario.txt
+++ b/scenario.txt
@@ -1 +1 @@
-base
+overlay
"#,
    )
    .expect("write variant overlay");
    let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
    fs::create_dir_all(&rig_dir).expect("mkdir rigs");
    fs::write(
        rig_dir.join(format!("{}.json", rig_id)),
        format!(
            r#"{{
                    "components": {{
                        "{component_id}": {{ "path": "{}" }}
                    }},
                    "trace_workloads": {{ "nodejs": [
                        "${{components.{component_id}.path}}/studio-app-create-site.trace.mjs"
                    ] }},
                    "trace_variants": {{
                        "fresh-install-mode": {{
                            "component": "{component_id}",
                            "overlay": "overlays/fresh-install-mode.patch"
                        }}
                    }}
                }}"#,
            path.display()
        ),
    )
    .expect("write rig");
}
