use homeboy::http_api::{self, HttpApiRequest, HttpEndpoint, HttpMethod, JobReadyRunKind};
use homeboy::observation::{NewRunRecord, ObservationStore, RunStatus};

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
fn routes_component_endpoints() {
    assert_eq!(
        http_api::route(HttpMethod::Get, "/components").expect("route"),
        HttpEndpoint::Components
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/components/homeboy").expect("route"),
        HttpEndpoint::Component {
            id: "homeboy".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/components/homeboy/status").expect("route"),
        HttpEndpoint::ComponentStatus {
            id: "homeboy".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/components/homeboy/changes?gitDiffs=1").expect("route"),
        HttpEndpoint::ComponentChanges {
            id: "homeboy".to_string()
        }
    );
}

#[test]
fn routes_rig_and_stack_endpoints() {
    assert_eq!(
        http_api::route(HttpMethod::Get, "/rigs/").expect("route"),
        HttpEndpoint::Rigs
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/rigs/studio").expect("route"),
        HttpEndpoint::Rig {
            id: "studio".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Post, "/rigs/studio/check").expect("route"),
        HttpEndpoint::RigCheck {
            id: "studio".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/stacks").expect("route"),
        HttpEndpoint::Stacks
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/stacks/studio").expect("route"),
        HttpEndpoint::Stack {
            id: "studio".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Post, "/stacks/studio/status").expect("route"),
        HttpEndpoint::StackStatus {
            id: "studio".to_string()
        }
    );
}

#[test]
fn routes_job_ready_analysis_endpoints_without_executing_them() {
    assert_eq!(
        http_api::route(HttpMethod::Post, "/audit").expect("route"),
        HttpEndpoint::JobReadyRun {
            kind: JobReadyRunKind::Audit
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Post, "/lint").expect("route"),
        HttpEndpoint::JobReadyRun {
            kind: JobReadyRunKind::Lint
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Post, "/test").expect("route"),
        HttpEndpoint::JobReadyRun {
            kind: JobReadyRunKind::Test
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Post, "/bench").expect("route"),
        HttpEndpoint::JobReadyRun {
            kind: JobReadyRunKind::Bench
        }
    );
}

#[test]
fn routes_observation_run_readers() {
    assert_eq!(
        http_api::route(HttpMethod::Get, "/runs?kind=bench").expect("route"),
        HttpEndpoint::Runs
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/runs/run-123").expect("route"),
        HttpEndpoint::Run {
            id: "run-123".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/runs/run-123/artifacts").expect("route"),
        HttpEndpoint::RunArtifacts {
            id: "run-123".to_string()
        }
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/audit/runs").expect("route"),
        HttpEndpoint::AuditRuns
    );
    assert_eq!(
        http_api::route(HttpMethod::Get, "/bench/runs").expect("route"),
        HttpEndpoint::BenchRuns
    );
}

#[test]
fn handles_filtered_observation_run_readers_without_starting_jobs() {
    with_isolated_home(|home| {
        let _xdg = XdgGuard::unset();
        let store = ObservationStore::open_initialized().expect("store");
        let bench = store
            .start_run(sample_run("bench", "homeboy", "studio"))
            .expect("bench run");
        store
            .finish_run(&bench.id, RunStatus::Pass, None)
            .expect("finish bench");
        let audit = store
            .start_run(sample_run("audit", "homeboy", "studio"))
            .expect("audit run");
        store
            .finish_run(&audit.id, RunStatus::Fail, None)
            .expect("finish audit");
        let artifact_path = home.path().join("bench-results.json");
        std::fs::write(&artifact_path, b"{}").expect("artifact");
        store
            .record_artifact(&bench.id, "bench_results", &artifact_path)
            .expect("record artifact");

        let response = http_api::handle(HttpApiRequest {
            method: HttpMethod::Get,
            path: "/bench/runs?component=homeboy&rig=studio&limit=1".to_string(),
            body: None,
        })
        .expect("bench runs");
        assert_eq!(response.endpoint, "bench.runs");
        assert_eq!(response.body["runs"].as_array().unwrap().len(), 1);
        assert_eq!(response.body["runs"][0]["id"], bench.id);

        let response = http_api::handle(HttpApiRequest {
            method: HttpMethod::Get,
            path: format!("/runs/{}", bench.id),
            body: None,
        })
        .expect("show run");
        assert_eq!(response.endpoint, "runs.show");
        assert_eq!(response.body["run"]["id"], bench.id);
        assert_eq!(
            response.body["run"]["artifacts"][0]["kind"],
            "bench_results"
        );

        let response = http_api::handle(HttpApiRequest {
            method: HttpMethod::Get,
            path: "/audit/runs?component=homeboy".to_string(),
            body: None,
        })
        .expect("audit runs");
        assert_eq!(response.endpoint, "audit.runs");
        assert_eq!(response.body["runs"].as_array().unwrap().len(), 1);
        assert_eq!(response.body["runs"][0]["id"], audit.id);
    });
}

#[test]
fn rejects_mutating_endpoint_shapes() {
    assert!(http_api::route(HttpMethod::Post, "/rigs/studio/up").is_err());
    assert!(http_api::route(HttpMethod::Post, "/stacks/studio/apply").is_err());
    assert!(http_api::route(HttpMethod::Post, "/deploy").is_err());
    assert!(http_api::route(HttpMethod::Post, "/release").is_err());
}

#[test]
fn job_ready_endpoint_reports_job_model_blocker() {
    let err = http_api::handle(HttpApiRequest {
        method: HttpMethod::Post,
        path: "/audit".to_string(),
        body: None,
    })
    .expect_err("job model blocker");

    let rendered = err.to_string();
    assert!(rendered.contains("job model"), "{rendered}");
    assert!(rendered.contains("#1764"), "{rendered}");
}

fn sample_run(kind: &str, component_id: &str, rig_id: &str) -> NewRunRecord {
    NewRunRecord {
        kind: kind.to_string(),
        component_id: Some(component_id.to_string()),
        command: Some(format!("homeboy {kind}")),
        cwd: Some("/tmp/homeboy-fixture".to_string()),
        homeboy_version: Some("test-version".to_string()),
        git_sha: Some("abc123".to_string()),
        rig_id: Some(rig_id.to_string()),
        metadata_json: serde_json::json!({ "source": "http-api-test" }),
    }
}
