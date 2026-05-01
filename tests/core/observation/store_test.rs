//! Observation-store foundation tests.
//!
//! These isolate `HOME` / `XDG_DATA_HOME` so the developer's real local DB is
//! never read or written.

use crate::observation::store::{self, ObservationStore, CURRENT_SCHEMA_VERSION};
use crate::observation::{NewRunRecord, RunListFilter, RunStatus};
use crate::test_support::with_isolated_home;

struct XdgGuard {
    prior: Option<String>,
}

impl XdgGuard {
    fn unset() -> Self {
        let prior = std::env::var("XDG_DATA_HOME").ok();
        std::env::remove_var("XDG_DATA_HOME");
        Self { prior }
    }

    fn set(value: &std::path::Path) -> Self {
        let prior = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", value);
        Self { prior }
    }
}

impl Drop for XdgGuard {
    fn drop(&mut self) {
        match &self.prior {
            Some(value) => std::env::set_var("XDG_DATA_HOME", value),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
    }
}

#[test]
fn test_status() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();

        let status = store::status().expect("status");

        assert!(!status.exists);
        assert_eq!(status.schema_version, 0);
        assert_eq!(status.migration_count, 0);
        assert_eq!(status.table_count, 0);
        assert_eq!(
            status.path,
            home.path()
                .join(".local/share/homeboy/homeboy.sqlite")
                .to_string_lossy()
        );
        assert!(
            !std::path::Path::new(&status.path).exists(),
            "read-only status must not create the DB"
        );
    });
}

#[test]
fn test_database_path() {
    with_isolated_home(|home| {
        let data_home = home.path().join("xdg-data");
        let _xdg = XdgGuard::set(&data_home);

        let path = store::database_path().expect("db path");

        assert_eq!(path, data_home.join("homeboy/homeboy.sqlite"));
    });
}

#[test]
fn test_open_initialized() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();

        let store = ObservationStore::open_initialized().expect("init store");
        let status = store.status().expect("status");

        assert!(status.exists);
        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.migration_count, 1);
        assert_eq!(status.table_count, 3);
    });
}

#[test]
fn initialization_is_idempotent() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();

        ObservationStore::open_initialized().expect("first init");
        let second = ObservationStore::open_initialized().expect("second init");
        let status = second.status().expect("status");

        assert_eq!(status.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(status.migration_count, 1);
        assert_eq!(status.table_count, 3);
    });
}

fn sample_run(kind: &str, component_id: &str) -> NewRunRecord {
    NewRunRecord {
        kind: kind.to_string(),
        component_id: Some(component_id.to_string()),
        command: Some(format!("homeboy {kind} {component_id}")),
        cwd: Some("/tmp/homeboy-fixture".to_string()),
        homeboy_version: Some("test-version".to_string()),
        git_sha: Some("abc123".to_string()),
        rig_id: Some("studio".to_string()),
        metadata_json: serde_json::json!({
            "scenario": "fixture",
            "attempt": 1,
        }),
    }
}

#[test]
fn test_start_run() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");

        let started = store
            .start_run(sample_run("bench", "homeboy"))
            .expect("start run");

        assert_eq!(started.kind, "bench");
        assert_eq!(started.component_id.as_deref(), Some("homeboy"));
        assert_eq!(started.status, "running");
        assert!(started.finished_at.is_none());
        assert_eq!(started.metadata_json["scenario"], "fixture");

        let fetched = store
            .get_run(&started.id)
            .expect("get run")
            .expect("run exists");

        assert_eq!(fetched, started);
    });
}

#[test]
fn test_finish_run() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");
        let started = store
            .start_run(sample_run("bench", "homeboy"))
            .expect("start run");

        let finished = store
            .finish_run(
                &started.id,
                RunStatus::Pass,
                Some(serde_json::json!({ "scenario": "fixture", "ok": true })),
            )
            .expect("finish run");
        let fetched = store
            .get_run(&started.id)
            .expect("get run")
            .expect("run exists");

        assert_eq!(finished.status, "pass");
        assert!(finished.finished_at.is_some());
        assert_eq!(finished.metadata_json["ok"], true);
        assert_eq!(fetched, finished);
    });
}

#[test]
fn test_list_runs() {
    with_isolated_home(|_home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");

        let bench = store
            .start_run(sample_run("bench", "homeboy"))
            .expect("start bench");
        store
            .finish_run(&bench.id, RunStatus::Pass, None)
            .expect("finish bench");

        let mut trace = sample_run("trace", "homeboy");
        trace.rig_id = Some("other-rig".to_string());
        let trace = store.start_run(trace).expect("start trace");
        store
            .finish_run(&trace.id, RunStatus::Fail, None)
            .expect("finish trace");

        let filtered = store
            .list_runs(RunListFilter {
                kind: Some("bench".to_string()),
                component_id: Some("homeboy".to_string()),
                status: Some("pass".to_string()),
                rig_id: Some("studio".to_string()),
                limit: Some(10),
            })
            .expect("list filtered");

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, bench.id);
        assert_eq!(filtered[0].status, "pass");

        let missing = store
            .list_runs(RunListFilter {
                status: Some("error".to_string()),
                ..RunListFilter::default()
            })
            .expect("list missing");
        assert!(missing.is_empty());
    });
}

#[test]
fn test_record_artifact() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");
        let run = store
            .start_run(sample_run("trace", "homeboy"))
            .expect("start run");
        let artifact_path = home.path().join("trace-results.json");
        std::fs::write(&artifact_path, br#"{"status":"pass"}"#).expect("write artifact");

        let artifact = store
            .record_artifact(&run.id, "trace-results", &artifact_path)
            .expect("record artifact");
        let artifacts = store.list_artifacts(&run.id).expect("list artifacts");

        assert_eq!(artifacts, vec![artifact.clone()]);
        assert_eq!(artifact.run_id, run.id);
        assert_eq!(artifact.kind, "trace-results");
        assert_eq!(artifact.path, artifact_path.to_string_lossy());
        assert_eq!(artifact.size_bytes, Some(17));
        assert_eq!(artifact.mime.as_deref(), Some("application/json"));
        assert_eq!(
            artifact.sha256.as_deref(),
            Some("117367705c6e7ef5d779dd71de15a95ee62339e1ef635f08246f8e1ec99167e2")
        );
    });
}

#[test]
fn test_list_artifacts() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");
        let run = store
            .start_run(sample_run("trace", "homeboy"))
            .expect("start run");
        let first_path = home.path().join("first.json");
        let second_path = home.path().join("second.log");
        std::fs::write(&first_path, b"first").expect("write first");
        std::fs::write(&second_path, b"second").expect("write second");

        let first = store
            .record_artifact(&run.id, "first", &first_path)
            .expect("record first");
        let second = store
            .record_artifact(&run.id, "second", &second_path)
            .expect("record second");

        let artifacts = store.list_artifacts(&run.id).expect("list artifacts");
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].id, first.id);
        assert_eq!(artifacts[1].id, second.id);
    });
}

#[test]
fn missing_artifact_file_returns_clear_error() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("init store");
        let run = store
            .start_run(sample_run("bench", "homeboy"))
            .expect("start run");
        let missing = home.path().join("missing.json");

        let err = store
            .record_artifact(&run.id, "missing", &missing)
            .expect_err("missing artifact should fail");

        assert_eq!(err.code.as_str(), "validation.invalid_argument");
        assert!(err.message.contains("artifact file not found"));
        assert!(err.details.to_string().contains("missing.json"));
    });
}
