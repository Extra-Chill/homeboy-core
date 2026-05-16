use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{self, ConfigEntity};
use crate::engine::shell;
use crate::error::{Error, Result};
use crate::output::{CreateOutput, MergeOutput, RemoveResult};
use crate::paths;
use crate::server::{self, Server, SshClient};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Local,
    Ssh,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runner {
    #[serde(skip_deserializing, default)]
    pub id: String,
    pub kind: RunnerKind,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub homeboy_path: Option<String>,
    #[serde(default)]
    pub daemon: bool,
    #[serde(default)]
    pub concurrency_limit: Option<usize>,
    #[serde(default)]
    pub artifact_policy: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub resources: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunnerSession {
    pub runner_id: String,
    pub server_id: Option<String>,
    pub remote_daemon_address: String,
    pub local_port: u16,
    pub local_url: String,
    pub tunnel_pid: Option<u32>,
    pub remote_daemon_pid: Option<u32>,
    pub homeboy_version: String,
    pub connected_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunnerFailureKind {
    SshFailure,
    MissingRemoteHomeboy,
    DaemonStartupFailure,
    TunnelFailure,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerConnectReport {
    pub runner_id: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_daemon_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tunnel_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_daemon_pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homeboy_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<RunnerFailureKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerStatusReport {
    pub runner_id: String,
    pub connected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<RunnerSession>,
    pub session_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunnerDisconnectReport {
    pub runner_id: String,
    pub disconnected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<RunnerSession>,
    pub session_path: String,
}

#[derive(Debug, Clone, Deserialize)]
struct CliEnvelope {
    success: bool,
    data: Option<Value>,
    error: Option<Value>,
}

impl ConfigEntity for Runner {
    const ENTITY_TYPE: &'static str = "runner";
    const DIR_NAME: &'static str = "runners";

    fn id(&self) -> &str {
        &self.id
    }

    fn set_id(&mut self, id: String) {
        self.id = id;
    }

    fn not_found_error(id: String, suggestions: Vec<String>) -> Error {
        Error::runner_not_found(id, suggestions)
    }

    fn validate(&self) -> Result<()> {
        if matches!(self.kind, RunnerKind::Ssh) {
            let server_id = self.server_id.as_deref().ok_or_else(|| {
                Error::validation_invalid_argument(
                    "server_id",
                    "SSH runners require server_id",
                    None,
                    None,
                )
            })?;
            server::load(server_id)?;
        }

        if self.concurrency_limit == Some(0) {
            return Err(Error::validation_invalid_argument(
                "concurrency_limit",
                "concurrency_limit must be greater than zero",
                None,
                None,
            ));
        }

        Ok(())
    }

    fn dependents(_id: &str) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

entity_crud!(Runner; merge);

pub fn connect(runner_id: &str) -> Result<(RunnerConnectReport, i32)> {
    let runner = load(runner_id)?;
    let session_path = session_path(runner_id)?;
    let homeboy = runner.homeboy_path.as_deref().unwrap_or("homeboy");

    let Some((server_id, _server, client)) = resolve_ssh_runner(&runner)? else {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::SshFailure,
            "only SSH runners are supported by runner connect in this wave".to_string(),
        ));
    };

    let ssh_probe = client.execute("true");
    if !ssh_probe.success {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::SshFailure,
            command_failure_message("SSH connectivity check failed", &ssh_probe),
        ));
    }

    let version = remote_homeboy_version(&client, homeboy);
    let Ok(version) = version else {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::MissingRemoteHomeboy,
            version.err().unwrap(),
        ));
    };

    let daemon = ensure_remote_daemon(&client, homeboy);
    let Ok(daemon) = daemon else {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::DaemonStartupFailure,
            daemon.err().unwrap(),
        ));
    };

    let Ok(remote_addr) = parse_loopback_daemon_addr(&daemon.address) else {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::DaemonStartupFailure,
            "remote daemon did not report a loopback address".to_string(),
        ));
    };

    let local_port = reserve_loopback_port()?;
    let tunnel = client.open_loopback_tunnel(
        local_port,
        &remote_addr.ip().to_string(),
        remote_addr.port(),
    );
    if !tunnel.success {
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::TunnelFailure,
            format!("SSH tunnel setup failed: {}", tunnel.stderr.trim()),
        ));
    }

    if !wait_for_tcp(local_port, Duration::from_secs(5)) {
        if let Some(pid) = tunnel.pid {
            terminate_pid(pid);
        }
        return Ok(failed_connect(
            runner_id,
            session_path,
            RunnerFailureKind::TunnelFailure,
            format!(
                "local tunnel 127.0.0.1:{} did not become reachable",
                local_port
            ),
        ));
    }

    let session = RunnerSession {
        runner_id: runner.id.clone(),
        server_id: Some(server_id),
        remote_daemon_address: daemon.address,
        local_port,
        local_url: format!("http://127.0.0.1:{}", local_port),
        tunnel_pid: tunnel.pid,
        remote_daemon_pid: daemon.pid,
        homeboy_version: version,
        connected_at: Utc::now().to_rfc3339(),
    };
    write_session(&session)?;

    Ok((
        RunnerConnectReport {
            runner_id: runner.id,
            connected: true,
            local_url: Some(session.local_url.clone()),
            remote_daemon_address: Some(session.remote_daemon_address.clone()),
            tunnel_pid: session.tunnel_pid,
            remote_daemon_pid: session.remote_daemon_pid,
            homeboy_version: Some(session.homeboy_version.clone()),
            session_path: Some(session_path.display().to_string()),
            failure_kind: None,
            failure_message: None,
        },
        0,
    ))
}

pub fn status(runner_id: &str) -> Result<RunnerStatusReport> {
    load(runner_id)?;
    let session_path = session_path(runner_id)?;
    let session = read_session(runner_id)?;
    let connected = session.as_ref().is_some_and(session_is_live);
    Ok(RunnerStatusReport {
        runner_id: runner_id.to_string(),
        connected,
        session,
        session_path: session_path.display().to_string(),
    })
}

pub fn disconnect(runner_id: &str) -> Result<RunnerDisconnectReport> {
    load(runner_id)?;
    let session_path = session_path(runner_id)?;
    let session = read_session(runner_id)?;
    if let Some(session) = &session {
        if let Some(pid) = session.tunnel_pid {
            terminate_pid(pid);
        }
    }
    if session_path.exists() {
        std::fs::remove_file(&session_path).map_err(|err| {
            Error::internal_io(
                err.to_string(),
                Some(format!("delete {}", session_path.display())),
            )
        })?;
    }
    Ok(RunnerDisconnectReport {
        runner_id: runner_id.to_string(),
        disconnected: session.is_some(),
        session,
        session_path: session_path.display().to_string(),
    })
}

fn resolve_ssh_runner(runner: &Runner) -> Result<Option<(String, Server, SshClient)>> {
    if runner.kind != RunnerKind::Ssh {
        return Ok(None);
    }
    let server_id = runner.server_id.clone().ok_or_else(|| {
        Error::validation_invalid_argument(
            "server_id",
            "SSH runner requires server_id",
            Some(runner.id.clone()),
            None,
        )
    })?;
    let server = server::load(&server_id)?;
    let client = SshClient::from_server(&server, &server_id)?;
    Ok(Some((server_id, server, client)))
}

fn remote_homeboy_version(
    client: &SshClient,
    homeboy: &str,
) -> std::result::Result<String, String> {
    let command = format!("{} --version", shell::quote_arg(homeboy));
    let output = client.execute(&command);
    if !output.success {
        return Err(command_failure_message(
            "remote Homeboy version check failed",
            &output,
        ));
    }
    let version = output.stdout.trim().to_string();
    if version.is_empty() {
        return Err("remote Homeboy version check returned empty output".to_string());
    }
    Ok(version)
}

#[derive(Debug)]
struct RemoteDaemon {
    address: String,
    pid: Option<u32>,
}

fn ensure_remote_daemon(
    client: &SshClient,
    homeboy: &str,
) -> std::result::Result<RemoteDaemon, String> {
    if let Some(daemon) = remote_daemon_status(client, homeboy)? {
        return Ok(daemon);
    }
    remote_daemon_start(client, homeboy)
}

fn remote_daemon_status(
    client: &SshClient,
    homeboy: &str,
) -> std::result::Result<Option<RemoteDaemon>, String> {
    let command = format!("{} daemon status", shell::quote_arg(homeboy));
    let output = client.execute(&command);
    if !output.success {
        return Ok(None);
    }
    let envelope = parse_envelope(&output.stdout)
        .map_err(|err| format!("remote daemon status returned invalid JSON: {}", err))?;
    if !envelope.success {
        return Ok(None);
    }
    let Some(data) = envelope.data else {
        return Ok(None);
    };
    if !data
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(None);
    }
    let Some(state) = data.get("state") else {
        return Ok(None);
    };
    Ok(Some(RemoteDaemon {
        address: state
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        pid: state
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok()),
    }))
}

fn remote_daemon_start(
    client: &SshClient,
    homeboy: &str,
) -> std::result::Result<RemoteDaemon, String> {
    let command = format!(
        "{} daemon start --addr 127.0.0.1:0",
        shell::quote_arg(homeboy)
    );
    let output = client.execute(&command);
    if !output.success {
        return Err(command_failure_message(
            "remote daemon startup failed",
            &output,
        ));
    }
    let envelope = parse_envelope(&output.stdout)
        .map_err(|err| format!("remote daemon start returned invalid JSON: {}", err))?;
    if !envelope.success {
        return Err(format!(
            "remote daemon start failed: {}",
            envelope.error.unwrap_or(Value::Null)
        ));
    }
    let data = envelope
        .data
        .ok_or_else(|| "remote daemon start returned no data".to_string())?;
    Ok(RemoteDaemon {
        address: data
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        pid: data
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok()),
    })
}

fn parse_envelope(stdout: &str) -> serde_json::Result<CliEnvelope> {
    serde_json::from_str(stdout.trim())
}

fn parse_loopback_daemon_addr(address: &str) -> std::result::Result<SocketAddr, ()> {
    let addr: SocketAddr = address.parse().map_err(|_| ())?;
    if addr.ip().is_loopback() {
        Ok(addr)
    } else {
        Err(())
    }
}

fn reserve_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind((IpAddr::from([127, 0, 0, 1]), 0)).map_err(|err| {
        Error::internal_io(
            err.to_string(),
            Some("reserve local tunnel port".to_string()),
        )
    })?;
    let port = listener
        .local_addr()
        .map_err(|err| {
            Error::internal_io(err.to_string(), Some("read local tunnel port".to_string()))
        })?
        .port();
    drop(listener);
    Ok(port)
}

fn wait_for_tcp(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn session_is_live(session: &RunnerSession) -> bool {
    if let Some(pid) = session.tunnel_pid {
        if !pid_is_running(pid) {
            return false;
        }
    }
    wait_for_tcp(session.local_port, Duration::from_millis(200))
}

fn session_path(runner_id: &str) -> Result<PathBuf> {
    paths::runner_session_file(runner_id)
}

fn read_session(runner_id: &str) -> Result<Option<RunnerSession>> {
    let path = session_path(runner_id)?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|err| {
        Error::internal_io(err.to_string(), Some(format!("read {}", path.display())))
    })?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|err| Error::config_invalid_json(path.display().to_string(), err))
}

fn write_session(session: &RunnerSession) -> Result<()> {
    let path = session_path(&session.runner_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            Error::internal_io(
                err.to_string(),
                Some(format!("create {}", parent.display())),
            )
        })?;
    }
    let body = serde_json::to_string_pretty(session).map_err(|err| {
        Error::internal_json(
            err.to_string(),
            Some("serialize runner session".to_string()),
        )
    })?;
    std::fs::write(&path, body).map_err(|err| {
        Error::internal_io(err.to_string(), Some(format!("write {}", path.display())))
    })
}

fn failed_connect(
    runner_id: &str,
    session_path: PathBuf,
    failure_kind: RunnerFailureKind,
    failure_message: String,
) -> (RunnerConnectReport, i32) {
    (
        RunnerConnectReport {
            runner_id: runner_id.to_string(),
            connected: false,
            local_url: None,
            remote_daemon_address: None,
            tunnel_pid: None,
            remote_daemon_pid: None,
            homeboy_version: None,
            session_path: Some(session_path.display().to_string()),
            failure_kind: Some(failure_kind),
            failure_message: Some(failure_message),
        },
        20,
    )
}

fn command_failure_message(prefix: &str, output: &crate::server::CommandOutput) -> String {
    format!(
        "{} (exit {}): stdout={}, stderr={}",
        prefix,
        output.exit_code,
        output.stdout.trim(),
        output.stderr.trim()
    )
}

fn pid_is_running(pid: u32) -> bool {
    if pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as libc::pid_t, 0) == 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn terminate_pid(pid: u32) {
    if pid > i32::MAX as u32 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn runner_registry_persists_local_runner() {
        test_support::with_isolated_home(|_| {
            let spec = r#"{
                "id": "lab-local",
                "kind": "local",
                "workspace_root": "/Users/chubes/Developer",
                "homeboy_path": "/usr/local/bin/homeboy",
                "daemon": true,
                "concurrency_limit": 2,
                "artifact_policy": "copy",
                "env": {"RUST_LOG": "info"},
                "resources": {"cpu": 8}
            }"#;

            create(spec, false).expect("create runner");
            let runner = load("lab-local").expect("load runner");

            assert_eq!(runner.id, "lab-local");
            assert_eq!(runner.kind, RunnerKind::Local);
            assert_eq!(runner.server_id, None);
            assert_eq!(
                runner.workspace_root.as_deref(),
                Some("/Users/chubes/Developer")
            );
            assert_eq!(runner.concurrency_limit, Some(2));
            assert_eq!(runner.env.get("RUST_LOG").map(String::as_str), Some("info"));
            assert_eq!(runner.resources.get("cpu"), Some(&Value::from(8)));
        });
    }

    #[test]
    fn ssh_runner_requires_existing_server() {
        test_support::with_isolated_home(|_| {
            let spec = r#"{
                "id": "remote-lab",
                "kind": "ssh",
                "server_id": "missing",
                "workspace_root": "/srv/homeboy"
            }"#;

            let err = create(spec, false).expect_err("missing server rejects ssh runner");
            assert_eq!(err.code.as_str(), "server.not_found");
        });
    }

    #[test]
    fn runner_set_updates_fields() {
        test_support::with_isolated_home(|_| {
            create(
                r#"{"id":"lab-local","kind":"local","workspace_root":"/tmp/a"}"#,
                false,
            )
            .expect("create runner");

            let result = merge(
                Some("lab-local"),
                r#"{"workspace_root":"/tmp/b","concurrency_limit":3}"#,
                &[],
            )
            .expect("merge runner");

            match result {
                MergeOutput::Single(result) => {
                    assert_eq!(result.id, "lab-local");
                    assert!(result
                        .updated_fields
                        .contains(&"workspace_root".to_string()));
                    assert!(result
                        .updated_fields
                        .contains(&"concurrency_limit".to_string()));
                }
                MergeOutput::Bulk(_) => panic!("expected single merge"),
            }

            let runner = load("lab-local").expect("load runner");
            assert_eq!(runner.workspace_root.as_deref(), Some("/tmp/b"));
            assert_eq!(runner.concurrency_limit, Some(3));
        });
    }

    #[test]
    fn rejects_non_loopback_remote_daemon_address() {
        assert!(parse_loopback_daemon_addr("0.0.0.0:1234").is_err());
        assert!(parse_loopback_daemon_addr("127.0.0.1:1234").is_ok());
    }

    #[test]
    fn parses_daemon_status_envelope() {
        let envelope = parse_envelope(
            r#"{"success":true,"data":{"action":"status","running":true,"state":{"address":"127.0.0.1:49152","pid":123}}}"#,
        )
        .expect("parse envelope");

        assert!(envelope.success);
        assert_eq!(
            envelope
                .data
                .unwrap()
                .get("state")
                .unwrap()
                .get("address")
                .unwrap(),
            "127.0.0.1:49152"
        );
    }
}
