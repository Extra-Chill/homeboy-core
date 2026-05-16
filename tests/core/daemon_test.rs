use super::*;
use crate::api_jobs::JobStore;
use crate::observation::{NewRunRecord, ObservationStore};
use crate::test_support::HomeGuard;

#[test]
fn parse_bind_addr_defaults_to_loopback_shape() {
    let addr = parse_bind_addr(DEFAULT_ADDR).expect("parse default");

    assert!(addr.ip().is_loopback());
    assert_eq!(addr.port(), 0);
}

#[test]
fn parse_bind_addr_rejects_public_bind() {
    let err = parse_bind_addr("0.0.0.0:8080").expect_err("reject public bind");

    assert!(err.message.contains("loopback"));
}

#[test]
fn routes_health_version_and_config_paths() {
    let _home = HomeGuard::new();

    let health = route("GET", "/health");
    assert_eq!(health.status_code, 200);
    assert_eq!(health.body["status"], "ok");

    let version = route("GET", "/version");
    assert_eq!(version.status_code, 200);
    assert_eq!(version.body["version"], env!("CARGO_PKG_VERSION"));

    let paths = route("GET", "/config/paths");
    assert_eq!(paths.status_code, 200);
    assert!(paths.body["homeboy"]
        .as_str()
        .unwrap()
        .ends_with(".config/homeboy"));
    assert!(paths.body["daemon_state"]
        .as_str()
        .unwrap()
        .ends_with("daemon/state.json"));
    assert!(paths.body["daemon_jobs"]
        .as_str()
        .unwrap()
        .ends_with("daemon/jobs.json"));
}

#[test]
fn routes_read_only_http_api_contract() {
    let _home = HomeGuard::new();

    let components = route("GET", "/components");
    assert_eq!(components.status_code, 200);
    assert_eq!(components.body["endpoint"], "components.list");
    assert!(components.body["body"]["components"].is_array());

    let job_ready = route("POST", "/audit");
    assert_eq!(job_ready.status_code, 200);
    assert_eq!(job_ready.body["endpoint"], "jobs.required");
    assert_eq!(job_ready.body["body"]["command"], "api.audit.enqueue");
    assert!(job_ready.body["body"]["poll"]["job"]
        .as_str()
        .unwrap()
        .starts_with("/jobs/"));

    let runs = route("GET", "/runs?kind=bench&limit=1");
    assert_eq!(runs.status_code, 200);
    assert_eq!(runs.body["endpoint"], "runs.list");
    assert!(runs.body["body"]["runs"].is_array());

    let bench_runs = route("GET", "/bench/runs?component=homeboy");
    assert_eq!(bench_runs.status_code, 200);
    assert_eq!(bench_runs.body["endpoint"], "bench.runs");
    assert!(bench_runs.body["body"]["runs"].is_array());

    let findings = route("GET", "/runs/run-missing/findings");
    assert_eq!(findings.status_code, 404);
    assert_eq!(findings.body["error"], "validation.invalid_argument");
}

#[test]
fn routes_registered_artifact_downloads_and_sync_manifest() {
    let _home = HomeGuard::new();
    let home_path = std::path::PathBuf::from(std::env::var("HOME").expect("home"));
    let store = ObservationStore::open_initialized().expect("store");
    let run = store
        .start_run(NewRunRecord {
            kind: "bench".to_string(),
            component_id: Some("homeboy".to_string()),
            command: Some("homeboy bench".to_string()),
            cwd: Some("/tmp/homeboy-fixture".to_string()),
            homeboy_version: Some("test-version".to_string()),
            git_sha: Some("abc123".to_string()),
            rig_id: Some("studio".to_string()),
            metadata_json: serde_json::json!({}),
        })
        .expect("run");
    let artifact_path = home_path.join("bench-results.json");
    std::fs::write(&artifact_path, br#"{"ok":true}"#).expect("artifact");
    let artifact = store
        .record_artifact(&run.id, "bench_results", &artifact_path)
        .expect("record artifact");

    let download = route(
        "GET",
        &format!("/runs/{}/artifacts/{}", run.id, artifact.id),
    );
    assert_eq!(download.status_code, 200);
    assert!(download.artifact.is_some());
    assert_eq!(download.body["artifact"]["id"], artifact.id);
    assert_eq!(download.body["size_bytes"], 11);

    let sync = route("GET", &format!("/runs/{}/artifacts/sync", run.id));
    assert_eq!(sync.status_code, 200);
    assert!(sync.artifact.is_none());
    assert_eq!(sync.body["command"], "api.runs.artifacts.sync");
    assert_eq!(sync.body["artifacts"][0]["id"], artifact.id);
    assert_eq!(
        sync.body["artifacts"][0]["download_path"],
        format!("/runs/{}/artifacts/{}", run.id, artifact.id)
    );

    let raw_path = route(
        "GET",
        &format!("/runs/{}/artifacts/{}", run.id, artifact_path.display()),
    );
    assert_eq!(raw_path.status_code, 404);
}

#[test]
fn routes_job_inspection_against_daemon_job_store() {
    let store = JobStore::default();
    let job = store.create("lint");

    let list = route_with_job_store("GET", "/jobs", &store);
    assert_eq!(list.status_code, 200);
    assert_eq!(list.body["endpoint"], "jobs.list");
    assert_eq!(list.body["body"]["jobs"].as_array().unwrap().len(), 1);

    let show = route_with_job_store("GET", &format!("/jobs/{}", job.id), &store);
    assert_eq!(show.status_code, 200);
    assert_eq!(show.body["endpoint"], "jobs.show");
    assert_eq!(show.body["body"]["job"]["operation"], "lint");

    let events = route_with_job_store("GET", &format!("/jobs/{}/events", job.id), &store);
    assert_eq!(events.status_code, 200);
    assert_eq!(events.body["endpoint"], "jobs.events");
    assert_eq!(events.body["body"]["events"].as_array().unwrap().len(), 1);

    let cancel = route_with_job_store("POST", &format!("/jobs/{}/cancel", job.id), &store);
    assert_eq!(cancel.status_code, 200);
    assert_eq!(cancel.body["endpoint"], "jobs.cancel");
    assert_eq!(cancel.body["body"]["job"]["status"], "cancelled");
}

#[test]
fn routes_json_body_to_analysis_enqueue() {
    let store = JobStore::default();
    let response = route_with_job_store_and_body(
        "POST",
        "/lint",
        Some(serde_json::json!({
            "component": "missing-component",
            "path": "/tmp/homeboy-missing-component",
            "changed_since": "origin/main",
            "json_summary": true
        })),
        &store,
    );

    assert_eq!(response.status_code, 200);
    assert_eq!(response.body["endpoint"], "jobs.required");
    assert_eq!(response.body["body"]["command"], "api.lint.enqueue");
    assert_eq!(store.list().len(), 1);
}

#[test]
fn route_rejects_unknown_paths_and_methods() {
    assert_eq!(route("GET", "/missing").status_code, 404);
    assert_eq!(route("POST", "/health").status_code, 405);
    assert_eq!(route("POST", "/release").status_code, 404);
}

#[test]
fn status_is_not_running_without_state_file() {
    let _home = HomeGuard::new();

    let status = read_status().expect("status");
    assert!(!status.running);
    assert!(status.state.is_none());
    assert!(status.state_path.ends_with("daemon/state.json"));
}
