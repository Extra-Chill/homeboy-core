use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::api_jobs::JobStore;
use crate::error::{Error, Result};
use crate::http_api::{self, HttpMethod};
use crate::observation::{ArtifactRecord, ObservationStore, RunRecord};
use crate::paths;
use crate::source_snapshot::SourceSnapshot;
use sha2::{Digest, Sha256};

const PATCH_CAPTURE_EXCLUDES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    ".next",
    ".turbo",
    ".cache",
];

mod artifact_download;
pub use artifact_download::ArtifactDownload;

pub const DEFAULT_ADDR: &str = "127.0.0.1:0";

static DAEMON_JOB_STORE: OnceLock<JobStore> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct DaemonState {
    pub address: String,
    pub pid: u32,
    pub state_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DaemonStatus {
    pub running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<DaemonState>,
    pub state_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DaemonStartResult {
    pub pid: u32,
    pub address: String,
    pub state_path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DaemonStopResult {
    pub stopped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub state_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub body: serde_json::Value,
    pub artifact: Option<ArtifactDownload>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExecRequest {
    #[serde(default)]
    runner_id: Option<String>,
    cwd: String,
    command: Vec<String>,
    #[serde(default)]
    capture_patch: bool,
    #[serde(default)]
    source_snapshot: Option<SourceSnapshot>,
}

#[derive(Debug, Clone, Serialize)]
struct PatchCaptureReport {
    source_snapshot_id: Option<String>,
    runner_id: String,
    command: Vec<String>,
    remote_path: String,
    modified_files: Vec<String>,
    patch_artifact_id: Option<String>,
    patch_artifact_path: Option<String>,
    dirty_snapshot: bool,
    baseline_missing: bool,
}

pub fn parse_bind_addr(addr: &str) -> Result<SocketAddr> {
    let parsed: SocketAddr = addr.parse().map_err(|e| {
        Error::validation_invalid_argument(
            "addr",
            format!("Invalid daemon bind address `{}`: {}", addr, e),
            Some(addr.to_string()),
            Some(vec!["Use a host:port value like 127.0.0.1:0".to_string()]),
        )
    })?;

    if !parsed.ip().is_loopback() {
        return Err(Error::validation_invalid_argument(
            "addr",
            "Daemon MVP only accepts loopback bind addresses",
            Some(addr.to_string()),
            Some(vec!["Use 127.0.0.1:<port> or [::1]:<port>".to_string()]),
        ));
    }

    Ok(parsed)
}

pub fn state_path() -> Result<PathBuf> {
    paths::daemon_state_file()
}

pub fn read_status() -> Result<DaemonStatus> {
    let path = state_path()?;
    let state_path = path.display().to_string();

    if !path.exists() {
        return Ok(DaemonStatus {
            running: false,
            state: None,
            state_path,
        });
    }

    let content = fs::read_to_string(&path)
        .map_err(|e| Error::internal_io(e.to_string(), Some(format!("read {}", path.display()))))?;
    let state: DaemonState = serde_json::from_str(&content)
        .map_err(|e| Error::config_invalid_json(path.display().to_string(), e))?;

    Ok(DaemonStatus {
        running: pid_is_running(state.pid),
        state: Some(state),
        state_path,
    })
}

pub fn stop() -> Result<DaemonStopResult> {
    let status = read_status()?;
    let Some(state) = status.state else {
        return Ok(DaemonStopResult {
            stopped: false,
            pid: None,
            state_path: status.state_path,
        });
    };

    let stopped = if pid_is_running(state.pid) {
        terminate_pid(state.pid)?;
        true
    } else {
        false
    };

    let path = state_path()?;
    if path.exists() {
        fs::remove_file(&path).map_err(|e| {
            Error::internal_io(e.to_string(), Some(format!("delete {}", path.display())))
        })?;
    }

    Ok(DaemonStopResult {
        stopped,
        pid: Some(state.pid),
        state_path: status.state_path,
    })
}

pub fn serve(addr: SocketAddr) -> Result<DaemonState> {
    let listener = TcpListener::bind(addr)
        .map_err(|e| Error::internal_io(e.to_string(), Some(format!("bind daemon to {}", addr))))?;
    let local_addr = listener.local_addr().map_err(|e| {
        Error::internal_io(e.to_string(), Some("read daemon local address".to_string()))
    })?;
    let state = write_state(local_addr)?;
    let job_store = JobStore::open(paths::daemon_jobs_file()?)?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let _ = handle_connection(stream, &job_store);
            }
            Err(err) => {
                return Err(Error::internal_io(
                    err.to_string(),
                    Some("accept daemon connection".to_string()),
                ));
            }
        }
    }

    Ok(state)
}

pub fn route(method: &str, path: &str) -> HttpResponse {
    route_with_job_store(method, path, daemon_job_store())
}

pub fn route_with_job_store(method: &str, path: &str, job_store: &JobStore) -> HttpResponse {
    route_with_job_store_and_body(method, path, None, job_store)
}

pub fn route_with_body(method: &str, path: &str, body: Option<serde_json::Value>) -> HttpResponse {
    route_with_job_store_and_body(method, path, body, daemon_job_store())
}

pub fn route_with_job_store_and_body(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    job_store: &JobStore,
) -> HttpResponse {
    match (method, path) {
        ("GET", "/health") => HttpResponse {
            status_code: 200,
            body: json!({
                "status": "ok",
                "version": env!("CARGO_PKG_VERSION"),
            }),
            artifact: None,
        },
        ("GET", "/version") => HttpResponse {
            status_code: 200,
            body: json!({
                "version": env!("CARGO_PKG_VERSION"),
            }),
            artifact: None,
        },
        ("GET", "/config/paths") => match config_paths_body() {
            Ok(body) => HttpResponse {
                status_code: 200,
                body,
                artifact: None,
            },
            Err(err) => error_response(500, err),
        },
        ("POST", "/health") | ("POST", "/version") | ("POST", "/config/paths") => HttpResponse {
            status_code: 405,
            body: json!({ "error": "method_not_allowed" }),
            artifact: None,
        },
        ("POST", "/exec") => match enqueue_exec_job(body, job_store) {
            Ok(body) => HttpResponse {
                status_code: 200,
                body: json!({
                    "status": 200,
                    "endpoint": "jobs.exec",
                    "body": body,
                }),
                artifact: None,
            },
            Err(err) => error_response(400, err),
        },
        ("GET", "/exec") => HttpResponse {
            status_code: 405,
            body: json!({ "error": "method_not_allowed" }),
            artifact: None,
        },
        _ => route_read_only_api(method, path, body, job_store),
    }
}

fn enqueue_exec_job(
    body: Option<serde_json::Value>,
    job_store: &JobStore,
) -> Result<serde_json::Value> {
    let request: ExecRequest =
        serde_json::from_value(body.unwrap_or_else(|| json!({}))).map_err(|err| {
            Error::validation_invalid_argument(
                "body",
                format!("invalid exec request body: {err}"),
                None,
                None,
            )
        })?;
    if request.command.is_empty() {
        return Err(Error::validation_invalid_argument(
            "command",
            "exec request requires command array",
            None,
            None,
        ));
    }
    if request.cwd.is_empty() || !Path::new(&request.cwd).is_absolute() {
        return Err(Error::validation_invalid_argument(
            "cwd",
            "exec request requires an absolute cwd",
            Some(request.cwd),
            None,
        ));
    }

    let runner_id = request.runner_id.as_deref().unwrap_or("unknown");
    let source_snapshot = request.source_snapshot.clone().or_else(|| {
        Some(SourceSnapshot::existing_remote(
            runner_id,
            &request.cwd,
            None,
        ))
    });

    let summary = json!({
        "runner_id": request.runner_id,
        "cwd": request.cwd,
        "command": request.command,
        "capture_patch": request.capture_patch,
        "source_snapshot": source_snapshot,
    });
    let operation = "runner.exec".to_string();
    let runner = job_store.run_background_with_source_snapshot(
        operation,
        source_snapshot.clone(),
        move |job| {
            job.progress(json!({
                "phase": "started",
                "runner_id": request.runner_id,
                "cwd": request.cwd,
                "command": request.command,
                "capture_patch": request.capture_patch,
                "job_id": job.job_id(),
                "source_snapshot": source_snapshot,
            }))?;
            let baseline = if request.capture_patch {
                Some(capture_baseline(&request.cwd)?)
            } else {
                None
            };
            let mut command = Command::new(&request.command[0]);
            command
                .args(&request.command[1..])
                .current_dir(&request.cwd);
            if let Some(snapshot) = &source_snapshot {
                command.env(
                    "HOMEBOY_SOURCE_SNAPSHOT_JSON",
                    serde_json::to_string(snapshot).unwrap_or_default(),
                );
            }
            let output = command.output().map_err(|err| {
                Error::internal_io(
                    err.to_string(),
                    Some("execute daemon runner command".to_string()),
                )
            })?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(1);
            if !stdout.is_empty() {
                job.stdout(stdout.clone())?;
            }
            if !stderr.is_empty() {
                job.stderr(stderr.clone())?;
            }
            job.progress(json!({
                "phase": "finished",
                "exit_code": exit_code,
            }))?;
            let patch = if let Some(baseline) = baseline {
                Some(capture_patch_report(
                    job.job_id(),
                    request.runner_id.as_deref().unwrap_or("unknown"),
                    &request.cwd,
                    &request.command,
                    source_snapshot.as_ref(),
                    &baseline,
                    exit_code,
                )?)
            } else {
                None
            };
            Ok(json!({
                "runner_id": request.runner_id,
                "cwd": request.cwd,
                "command": request.command,
                "exit_code": exit_code,
                "stdout": stdout,
                "stderr": stderr,
                "source_snapshot": source_snapshot,
                "patch": patch,
            }))
        },
    );
    let job = job_store.get(runner.job_id)?;

    Ok(json!({
        "command": "api.runner.exec.enqueue",
        "job": job,
        "poll": {
            "job": format!("/jobs/{}", runner.job_id),
            "events": format!("/jobs/{}/events", runner.job_id),
        },
        "request": summary,
    }))
}

struct BaselineCapture {
    _scratch: ScratchDir,
    path: PathBuf,
}

struct ScratchDir {
    path: PathBuf,
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct PatchRunInput<'a> {
    run_id: &'a str,
    runner_id: &'a str,
    cwd: &'a str,
    command: &'a [String],
    source_snapshot: Option<&'a SourceSnapshot>,
    report: &'a PatchCaptureReport,
    patch_artifact_path: Option<&'a Path>,
    artifact_id: &'a str,
    exit_code: i32,
}

fn capture_baseline(cwd: &str) -> Result<BaselineCapture> {
    let cwd_path = Path::new(cwd);
    if !cwd_path.is_dir() {
        return Err(Error::validation_invalid_argument(
            "cwd",
            "patch capture requires an existing directory baseline",
            Some(cwd.to_string()),
            None,
        ));
    }
    let scratch = create_scratch_dir("baseline")?;
    let baseline_path = scratch.path.join("baseline");
    copy_dir_filtered(cwd_path, &baseline_path)?;
    Ok(BaselineCapture {
        _scratch: scratch,
        path: baseline_path,
    })
}

fn capture_patch_report(
    job_id: uuid::Uuid,
    runner_id: &str,
    cwd: &str,
    command: &[String],
    source_snapshot: Option<&SourceSnapshot>,
    baseline: &BaselineCapture,
    exit_code: i32,
) -> Result<PatchCaptureReport> {
    let after_scratch = create_scratch_dir("after")?;
    let after_path = after_scratch.path.join("after");
    copy_dir_filtered(Path::new(cwd), &after_path)?;

    let patch = normalized_no_index_diff(&baseline.path, &after_path)?;
    let modified_files = no_index_modified_files(&baseline.path, &after_path)?;
    let run_id = format!("runner-exec-{job_id}");
    let artifact_id = format!("runner-fix-patch-{job_id}");
    let patch_artifact_path = if patch.trim().is_empty() {
        None
    } else {
        Some(write_patch_artifact(&run_id, &artifact_id, &patch)?)
    };
    let patch_artifact_path_string = patch_artifact_path
        .as_ref()
        .map(|path| path.display().to_string());
    let report = PatchCaptureReport {
        source_snapshot_id: source_snapshot.map(|snapshot| snapshot.snapshot_hash.clone()),
        runner_id: runner_id.to_string(),
        command: command.to_vec(),
        remote_path: cwd.to_string(),
        modified_files,
        patch_artifact_id: patch_artifact_path.as_ref().map(|_| artifact_id.clone()),
        patch_artifact_path: patch_artifact_path_string.clone(),
        dirty_snapshot: source_snapshot
            .map(|snapshot| snapshot.dirty)
            .unwrap_or(false),
        baseline_missing: false,
    };
    persist_patch_run(PatchRunInput {
        run_id: &run_id,
        runner_id,
        cwd,
        command,
        source_snapshot,
        report: &report,
        patch_artifact_path: patch_artifact_path.as_deref(),
        artifact_id: &artifact_id,
        exit_code,
    })?;
    Ok(report)
}

fn create_scratch_dir(label: &str) -> Result<ScratchDir> {
    let path = paths::artifact_root()?
        .join("_scratch")
        .join(format!("patch-{label}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).map_err(|err| {
        Error::internal_io(
            err.to_string(),
            Some(format!("create scratch directory {}", path.display())),
        )
    })?;
    Ok(ScratchDir { path })
}

fn write_patch_artifact(run_id: &str, artifact_id: &str, patch: &str) -> Result<PathBuf> {
    let path = paths::artifact_root()?
        .join(run_id)
        .join(format!("{artifact_id}.diff"));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Error::internal_io(
                err.to_string(),
                Some(format!("create {}", parent.display())),
            )
        })?;
    }
    fs::write(&path, patch).map_err(|err| {
        Error::internal_io(
            err.to_string(),
            Some(format!("write patch artifact {}", path.display())),
        )
    })?;
    Ok(path)
}

fn persist_patch_run(input: PatchRunInput<'_>) -> Result<()> {
    let store = ObservationStore::open_initialized()?;
    let now = chrono::Utc::now().to_rfc3339();
    let run = RunRecord {
        id: input.run_id.to_string(),
        kind: "runner-exec".to_string(),
        component_id: None,
        started_at: now.clone(),
        finished_at: Some(now.clone()),
        status: if input.exit_code == 0 { "pass" } else { "fail" }.to_string(),
        command: Some(input.command.join(" ")),
        cwd: Some(input.cwd.to_string()),
        homeboy_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        git_sha: input
            .source_snapshot
            .and_then(|snapshot| snapshot.git_sha.clone()),
        rig_id: None,
        metadata_json: json!({
            "lab": {
                "runner_id": input.runner_id,
                "source_snapshot": input.source_snapshot,
                "patch": input.report,
            }
        }),
    };
    if store.get_run(input.run_id)?.is_none() {
        store.import_run(&run)?;
    }
    if let Some(path) = input.patch_artifact_path {
        let bytes = fs::read(path).map_err(|err| {
            Error::internal_io(err.to_string(), Some(format!("read {}", path.display())))
        })?;
        let artifact = ArtifactRecord {
            id: input.artifact_id.to_string(),
            run_id: input.run_id.to_string(),
            kind: "lab_fix_patch".to_string(),
            artifact_type: "file".to_string(),
            path: path.display().to_string(),
            url: None,
            sha256: Some(format!("{:x}", Sha256::digest(&bytes))),
            size_bytes: i64::try_from(bytes.len()).ok(),
            mime: Some("text/x-diff".to_string()),
            created_at: now,
        };
        if store.get_artifact(input.artifact_id)?.is_none() {
            store.import_artifact(&artifact)?;
        }
    }
    Ok(())
}

fn normalized_no_index_diff(baseline: &Path, after: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "--no-index", "--binary", "--"])
        .arg(baseline)
        .arg(after)
        .output()
        .map_err(|err| {
            Error::internal_io(err.to_string(), Some("run git diff --no-index".to_string()))
        })?;
    let code = output.status.code().unwrap_or(1);
    if code > 1 {
        return Err(Error::internal_unexpected(format!(
            "git diff --no-index failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(normalize_patch_paths(
        &String::from_utf8_lossy(&output.stdout),
        baseline,
        after,
    ))
}

fn no_index_modified_files(baseline: &Path, after: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--no-index", "--name-only", "--"])
        .arg(baseline)
        .arg(after)
        .output()
        .map_err(|err| {
            Error::internal_io(
                err.to_string(),
                Some("run git diff --name-only".to_string()),
            )
        })?;
    let code = output.status.code().unwrap_or(1);
    if code > 1 {
        return Err(Error::internal_unexpected(format!(
            "git diff --name-only failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let mut files = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let path = Path::new(line);
        let relative = path
            .strip_prefix(after)
            .or_else(|_| path.strip_prefix(baseline))
            .unwrap_or(path)
            .to_string_lossy()
            .trim_start_matches('/')
            .to_string();
        if !relative.is_empty() {
            files.insert(relative);
        }
    }
    Ok(files.into_iter().collect())
}

fn normalize_patch_paths(patch: &str, baseline: &Path, after: &Path) -> String {
    let baseline = baseline.to_string_lossy();
    let after = after.to_string_lossy();
    patch
        .replace(&format!("a/{baseline}"), "a")
        .replace(&format!("b/{after}"), "b")
        .replace(baseline.as_ref(), "a")
        .replace(after.as_ref(), "b")
}

fn copy_dir_filtered(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target).map_err(|err| {
        Error::internal_io(
            err.to_string(),
            Some(format!("create {}", target.display())),
        )
    })?;
    for entry in fs::read_dir(source).map_err(|err| {
        Error::internal_io(err.to_string(), Some(format!("read {}", source.display())))
    })? {
        let entry = entry.map_err(|err| {
            Error::internal_io(err.to_string(), Some("read directory entry".to_string()))
        })?;
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if PATCH_CAPTURE_EXCLUDES.contains(&name_str) {
            continue;
        }
        let source_path = entry.path();
        let target_path = target.join(&name);
        let metadata = entry.metadata().map_err(|err| {
            Error::internal_io(
                err.to_string(),
                Some(format!("read metadata {}", source_path.display())),
            )
        })?;
        if metadata.is_dir() {
            copy_dir_filtered(&source_path, &target_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &target_path).map_err(|err| {
                Error::internal_io(
                    err.to_string(),
                    Some(format!("copy {}", source_path.display())),
                )
            })?;
        }
    }
    Ok(())
}

fn daemon_job_store() -> &'static JobStore {
    DAEMON_JOB_STORE.get_or_init(JobStore::default)
}

fn route_read_only_api(
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
    job_store: &JobStore,
) -> HttpResponse {
    let method = match method {
        "GET" => HttpMethod::Get,
        "POST" => HttpMethod::Post,
        _ => {
            return HttpResponse {
                status_code: 405,
                body: json!({ "error": "method_not_allowed" }),
                artifact: None,
            };
        }
    };

    if matches!(method, HttpMethod::Get) {
        if let Some(response) = artifact_download::route(path) {
            return response;
        }
    }

    match http_api::handle_with_jobs(
        http_api::HttpApiRequest {
            method,
            path: path.to_string(),
            body,
        },
        job_store,
    ) {
        Ok(response) => HttpResponse {
            status_code: response.status,
            body: serde_json::to_value(response)
                .unwrap_or_else(|_| json!({ "error": "internal_json" })),
            artifact: None,
        },
        Err(err) => error_response(404, err),
    }
}

fn error_response(status_code: u16, err: Error) -> HttpResponse {
    HttpResponse {
        status_code,
        body: json!({
            "error": err.code.as_str(),
            "message": err.message,
            "details": err.details,
            "hints": err.hints,
        }),
        artifact: None,
    }
}

fn config_paths_body() -> Result<serde_json::Value> {
    Ok(json!({
        "homeboy": paths::homeboy()?.display().to_string(),
        "homeboy_json": paths::homeboy_json()?.display().to_string(),
        "projects": paths::projects()?.display().to_string(),
        "servers": paths::servers()?.display().to_string(),
        "components": paths::components()?.display().to_string(),
        "extensions": paths::extensions()?.display().to_string(),
        "rigs": paths::rigs()?.display().to_string(),
        "stacks": paths::stacks()?.display().to_string(),
        "daemon_state": paths::daemon_state_file()?.display().to_string(),
        "daemon_jobs": paths::daemon_jobs_file()?.display().to_string(),
    }))
}

fn write_state(addr: SocketAddr) -> Result<DaemonState> {
    let path = state_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Error::internal_io(e.to_string(), Some(format!("create {}", parent.display())))
        })?;
    }

    let state = DaemonState {
        address: addr.to_string(),
        pid: std::process::id(),
        state_path: path.display().to_string(),
    };
    let body = serde_json::to_string_pretty(&state).map_err(|e| {
        Error::internal_json(e.to_string(), Some("serialize daemon state".to_string()))
    })?;
    fs::write(&path, body).map_err(|e| {
        Error::internal_io(e.to_string(), Some(format!("write {}", path.display())))
    })?;
    Ok(state)
}

fn handle_connection(mut stream: TcpStream, job_store: &JobStore) -> std::io::Result<()> {
    let mut buffer = [0; 64 * 1024];
    let bytes = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..bytes]);
    let mut headers_and_body = request.splitn(2, "\r\n\r\n");
    let headers = headers_and_body.next().unwrap_or_default();
    let body = headers_and_body.next().unwrap_or_default();
    let mut parts = headers
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default();
    let parsed_body = if body.trim().is_empty() {
        None
    } else {
        match serde_json::from_str::<serde_json::Value>(body.trim()) {
            Ok(value) => Some(value),
            Err(error) => {
                let response = error_response(
                    400,
                    Error::validation_invalid_argument(
                        "body",
                        format!("invalid JSON request body: {error}"),
                        None,
                        None,
                    ),
                );
                return write_http_response(stream, response);
            }
        }
    };
    let response = route_with_job_store_and_body(method, path, parsed_body, job_store);
    write_http_response(stream, response)
}

fn write_http_response(mut stream: TcpStream, response: HttpResponse) -> std::io::Result<()> {
    if let Some(artifact) = response.artifact {
        return artifact_download::write_response(stream, response.status_code, artifact);
    }

    let body = serde_json::to_string_pretty(&json!({
        "success": (200..300).contains(&response.status_code),
        "data": response.body,
    }))
    .unwrap_or_else(|_| "{\"success\":false}".to_string());
    let status_text = match response.status_code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status_code,
        status_text,
        body.len(),
        body
    )
}

pub(crate) fn pid_is_running(pid: u32) -> bool {
    if pid > i32::MAX as u32 {
        return false;
    }

    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
    }

    #[cfg(not(unix))]
    {
        pid == std::process::id()
    }
}

fn terminate_pid(pid: u32) -> Result<()> {
    #[cfg(unix)]
    unsafe {
        if libc::kill(pid as libc::pid_t, libc::SIGTERM) != 0 {
            return Err(Error::internal_io(
                std::io::Error::last_os_error().to_string(),
                Some(format!("stop daemon pid {}", pid)),
            ));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(Error::internal_unexpected(
            "daemon stop is not implemented on this platform",
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/core/daemon_test.rs"]
mod daemon_test;
