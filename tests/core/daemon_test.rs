use super::*;
use crate::api_jobs::JobStore;
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
}

#[test]
fn routes_read_only_http_api_contract() {
    let _home = HomeGuard::new();

    let components = route("GET", "/components");
    assert_eq!(components.status_code, 200);
    assert_eq!(components.body["endpoint"], "components.list");
    assert!(components.body["body"]["components"].is_array());

    let job_ready = route("POST", "/audit");
    assert_eq!(job_ready.status_code, 404);
    assert_eq!(job_ready.body["error"], "validation.invalid_argument");
    assert!(job_ready.body["message"]
        .as_str()
        .unwrap()
        .contains("analysis enqueue"));

    let runs = route("GET", "/runs?kind=bench&limit=1");
    assert_eq!(runs.status_code, 200);
    assert_eq!(runs.body["endpoint"], "runs.list");
    assert!(runs.body["body"]["runs"].is_array());

    let bench_runs = route("GET", "/bench/runs?component=homeboy");
    assert_eq!(bench_runs.status_code, 200);
    assert_eq!(bench_runs.body["endpoint"], "bench.runs");
    assert!(bench_runs.body["body"]["runs"].is_array());
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
