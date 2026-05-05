//! Per-child workload invocation isolation.

use crate::engine::run_dir::RunDir;
use crate::error::{Error, Result};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

const LOCK_NAME: &str = ".index.lock";
const LOCK_STALE_AFTER: Duration = Duration::from_secs(30);
const LOCK_ATTEMPTS: usize = 100;
const LOCK_SLEEP: Duration = Duration::from_millis(20);
const PORT_POOL_START: u16 = 20_000;
const PORT_POOL_END: u16 = 60_999;
const CHILD_RECORD_DIR: &str = "invocation-children";
const CHILD_CLEANUP_GRACE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InvocationRequirements {
    pub port_range_size: Option<u16>,
    pub named_leases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationEnv {
    pub id: String,
    pub state_dir: PathBuf,
    pub artifact_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub port_base: Option<u16>,
    pub port_max: Option<u16>,
}

#[derive(Debug)]
pub struct InvocationGuard {
    env: InvocationEnv,
    lease_id: Option<String>,
}

#[derive(Debug)]
pub struct InvocationChildGuard {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InvocationLease {
    invocation_id: String,
    pid: u32,
    started_at: String,
    port_base: Option<u16>,
    port_max: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    named_leases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationChildRecord {
    pub invocation_id: String,
    pub owner_pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_started_at: Option<String>,
    pub root_pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pgid: Option<i32>,
    pub command_label: String,
    pub started_at: String,
}

impl InvocationGuard {
    pub fn acquire(run_dir: &RunDir, requirements: &InvocationRequirements) -> Result<Self> {
        cleanup_stale_child_records()?;

        let id = format!("inv-{}", uuid::Uuid::new_v4());
        let root = run_dir.path().join("invocations").join(&id);
        let state_dir = root.join("state");
        let artifact_dir = root.join("artifacts");
        let tmp_dir = root.join("tmp");
        for dir in [&state_dir, &artifact_dir, &tmp_dir] {
            fs::create_dir_all(dir).map_err(|e| {
                Error::internal_io(
                    format!("Failed to create invocation dir {}: {}", dir.display(), e),
                    Some("invocation.dir".to_string()),
                )
            })?;
        }

        let needs_lease =
            requirements.port_range_size.is_some() || !requirements.named_leases.is_empty();
        let mut port_base = None;
        let mut port_max = None;
        let mut lease_id = None;

        if needs_lease {
            let _lock = InvocationIndexLock::acquire()?;
            fs::create_dir_all(invocation_leases_dir()?).map_err(|e| {
                Error::internal_unexpected(format!(
                    "Failed to create invocation lease directory: {}",
                    e
                ))
            })?;
            let live_leases = refresh_lease_index()?;
            validate_named_leases(&id, &requirements.named_leases)?;

            if let Some(size) = requirements.port_range_size {
                let (base, max) = allocate_port_range(size, &live_leases)?;
                port_base = Some(base);
                port_max = Some(max);
            }

            let lease = InvocationLease {
                invocation_id: id.clone(),
                pid: std::process::id(),
                started_at: chrono::Utc::now().to_rfc3339(),
                port_base,
                port_max,
                named_leases: requirements.named_leases.clone(),
            };
            write_lease(&lease)?;
            lease_id = Some(id.clone());
        }

        Ok(Self {
            env: InvocationEnv {
                id,
                state_dir,
                artifact_dir,
                tmp_dir,
                port_base,
                port_max,
            },
            lease_id,
        })
    }

    pub fn env_vars(&self) -> Vec<(String, String)> {
        let mut vars = vec![
            ("HOMEBOY_INVOCATION_ID".to_string(), self.env.id.clone()),
            (
                "HOMEBOY_INVOCATION_STATE_DIR".to_string(),
                self.env.state_dir.to_string_lossy().to_string(),
            ),
            (
                "HOMEBOY_INVOCATION_ARTIFACT_DIR".to_string(),
                self.env.artifact_dir.to_string_lossy().to_string(),
            ),
            (
                "HOMEBOY_INVOCATION_TMP_DIR".to_string(),
                self.env.tmp_dir.to_string_lossy().to_string(),
            ),
        ];
        if let (Some(base), Some(max)) = (self.env.port_base, self.env.port_max) {
            vars.push(("HOMEBOY_INVOCATION_PORT_BASE".to_string(), base.to_string()));
            vars.push(("HOMEBOY_INVOCATION_PORT_MAX".to_string(), max.to_string()));
        }
        vars
    }
}

impl Drop for InvocationChildGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn register_child_process(
    invocation_id: &str,
    root_pid: u32,
    pgid: Option<i32>,
    command_label: String,
) -> Result<InvocationChildGuard> {
    let dir = invocation_children_dir(invocation_id)?;
    fs::create_dir_all(&dir).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to create invocation child directory {}: {}",
                dir.display(),
                e
            ),
            Some("invocation.child.dir".to_string()),
        )
    })?;

    let record = InvocationChildRecord {
        invocation_id: invocation_id.to_string(),
        owner_pid: std::process::id(),
        owner_started_at: process_started_at(std::process::id()),
        root_pid,
        root_started_at: process_started_at(root_pid),
        pgid,
        command_label,
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = child_record_path(invocation_id, root_pid)?;
    let json = serde_json::to_string_pretty(&record).map_err(|e| {
        Error::internal_unexpected(format!(
            "Failed to serialize invocation child record: {}",
            e
        ))
    })?;
    fs::write(&path, json).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to write invocation child record {}: {}",
                path.display(),
                e
            ),
            Some("invocation.child.write".to_string()),
        )
    })?;

    Ok(InvocationChildGuard { path })
}

pub fn cleanup_invocation_children(invocation_id: &str) -> Result<usize> {
    let mut cleaned = 0;
    for path in invocation_child_files(invocation_id)? {
        let Some(record) = decode_child_record(&path)? else {
            continue;
        };
        if cleanup_child_record(&record) {
            cleaned += 1;
        }
        let _ = fs::remove_file(path);
    }
    Ok(cleaned)
}

pub fn cleanup_stale_child_records() -> Result<usize> {
    let mut cleaned = 0;
    let root = invocation_children_root()?;
    if !root.exists() {
        return Ok(0);
    }

    for entry in fs::read_dir(&root).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to read invocation child root {}: {}",
                root.display(),
                e
            ),
            Some("invocation.child.read".to_string()),
        )
    })? {
        let entry = entry.map_err(|e| {
            Error::internal_io(e.to_string(), Some("invocation.child.entry".to_string()))
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for child_path in child_files_in_dir(&path)? {
            let Some(record) = decode_child_record(&child_path)? else {
                let _ = fs::remove_file(child_path);
                continue;
            };
            if owner_is_gone(&record) {
                if cleanup_child_record(&record) {
                    cleaned += 1;
                }
                let _ = fs::remove_file(child_path);
            }
        }
    }
    Ok(cleaned)
}

impl Drop for InvocationGuard {
    fn drop(&mut self) {
        let Some(id) = &self.lease_id else {
            return;
        };
        let Ok(_lock) = InvocationIndexLock::acquire() else {
            return;
        };
        let Ok(path) = lease_path(id) else {
            return;
        };
        let Ok(Some(lease)) = decode_lease_file(&path) else {
            return;
        };
        if lease.pid == std::process::id() {
            let _ = fs::remove_file(path);
        }
    }
}

fn validate_named_leases(invocation_id: &str, wanted: &[String]) -> Result<()> {
    if wanted.is_empty() {
        return Ok(());
    }
    for lease in refresh_lease_index()? {
        for name in wanted {
            if lease.named_leases.contains(name) {
                return Err(Error::validation_invalid_argument(
                    "named_lease",
                    format!(
                        "Homeboy invocation lease '{}' is already held by invocation '{}' (pid {})",
                        name, lease.invocation_id, lease.pid
                    ),
                    Some(invocation_id.to_string()),
                    Some(vec![name.clone()]),
                ));
            }
        }
    }
    Ok(())
}

fn allocate_port_range(size: u16, live_leases: &[InvocationLease]) -> Result<(u16, u16)> {
    if size == 0 {
        return Err(Error::validation_invalid_argument(
            "port_range_size",
            "must be >= 1",
            None,
            None,
        ));
    }
    let size = size as u32;
    let pool_start = PORT_POOL_START as u32;
    let pool_end = PORT_POOL_END as u32;
    if size > pool_end - pool_start + 1 {
        return Err(Error::validation_invalid_argument(
            "port_range_size",
            format!("{} exceeds Homeboy invocation port pool capacity", size),
            None,
            None,
        ));
    }

    let mut ranges: Vec<(u32, u32)> = live_leases
        .iter()
        .filter_map(|lease| Some((lease.port_base? as u32, lease.port_max? as u32)))
        .collect();
    ranges.sort();

    let mut candidate = pool_start;
    for (base, max) in ranges {
        if candidate + size - 1 < base {
            return Ok((candidate as u16, (candidate + size - 1) as u16));
        }
        if candidate <= max {
            candidate = max + 1;
        }
    }

    if candidate + size - 1 <= pool_end {
        return Ok((candidate as u16, (candidate + size - 1) as u16));
    }

    Err(Error::validation_invalid_argument(
        "port_range_size",
        "no free Homeboy invocation port range is available on this machine",
        None,
        None,
    ))
}

fn refresh_lease_index() -> Result<Vec<InvocationLease>> {
    let mut live = Vec::new();
    for path in invocation_lease_files()? {
        let Some(lease) = decode_lease_file(&path)? else {
            continue;
        };
        if crate::core::daemon::pid_is_running(lease.pid) {
            live.push(lease);
        } else {
            remove_stale_invocation_lease(&path)?;
        }
    }
    Ok(live)
}

fn invocation_lease_files() -> Result<Vec<PathBuf>> {
    let dir = invocation_leases_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| {
        Error::internal_unexpected(format!("Failed to read invocation lease directory: {}", e))
    })? {
        let entry = entry.map_err(|e| {
            Error::internal_unexpected(format!("Failed to read invocation lease entry: {}", e))
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn remove_stale_invocation_lease(path: &Path) -> Result<()> {
    fs::remove_file(path).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to remove stale invocation lease {}: {}",
                path.display(),
                e
            ),
            Some("invocation.lease.stale".to_string()),
        )
    })
}

fn decode_lease_file(path: &Path) -> Result<Option<InvocationLease>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| read_lease_error(path, e))?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    let parsed = serde_json::from_str::<InvocationLease>(&content).map_err(|e| {
        Error::validation_invalid_json(e, Some(parse_context(path)), Some(json_excerpt(&content)))
    })?;
    Ok(Some(parsed))
}

fn read_lease_error(path: &Path, error: std::io::Error) -> Error {
    Error::internal_unexpected(format!(
        "Failed to read invocation lease {}: {}",
        path.display(),
        error
    ))
}

fn parse_context(path: &Path) -> String {
    format!("parse invocation lease {}", path.display())
}

fn json_excerpt(content: &str) -> String {
    content.chars().take(200).collect()
}

fn write_lease(lease: &InvocationLease) -> Result<()> {
    let json = serde_json::to_string_pretty(lease).map_err(|e| {
        Error::internal_unexpected(format!("Failed to serialize invocation lease: {}", e))
    })?;
    fs::write(lease_path(&lease.invocation_id)?, json).map_err(|e| {
        Error::internal_unexpected(format!(
            "Failed to write invocation lease for '{}': {}",
            lease.invocation_id, e
        ))
    })
}

fn lease_path(invocation_id: &str) -> Result<PathBuf> {
    Ok(invocation_leases_dir()?.join(format!("{}.json", sanitize_id(invocation_id))))
}

fn invocation_leases_dir() -> Result<PathBuf> {
    Ok(paths::homeboy()?.join("invocation-leases"))
}

fn invocation_children_root() -> Result<PathBuf> {
    Ok(paths::homeboy()?.join(CHILD_RECORD_DIR))
}

fn invocation_children_dir(invocation_id: &str) -> Result<PathBuf> {
    Ok(invocation_children_root()?.join(sanitize_id(invocation_id)))
}

fn child_record_path(invocation_id: &str, root_pid: u32) -> Result<PathBuf> {
    Ok(invocation_children_dir(invocation_id)?.join(format!("{}.json", root_pid)))
}

fn invocation_child_files(invocation_id: &str) -> Result<Vec<PathBuf>> {
    child_files_in_dir(&invocation_children_dir(invocation_id)?)
}

fn child_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to read invocation child directory {}: {}",
                dir.display(),
                e
            ),
            Some("invocation.child.read".to_string()),
        )
    })? {
        let entry = entry.map_err(|e| {
            Error::internal_io(e.to_string(), Some("invocation.child.entry".to_string()))
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn decode_child_record(path: &Path) -> Result<Option<InvocationChildRecord>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to read invocation child record {}: {}",
                path.display(),
                e
            ),
            Some("invocation.child.read".to_string()),
        )
    })?;
    if content.trim().is_empty() {
        return Ok(None);
    }
    serde_json::from_str::<InvocationChildRecord>(&content)
        .map(Some)
        .map_err(|e| {
            Error::validation_invalid_json(
                e,
                Some(parse_context(path)),
                Some(json_excerpt(&content)),
            )
        })
}

fn owner_is_gone(record: &InvocationChildRecord) -> bool {
    !process_identity_matches(record.owner_pid, record.owner_started_at.as_deref())
}

fn cleanup_child_record(record: &InvocationChildRecord) -> bool {
    if !process_identity_matches(record.root_pid, record.root_started_at.as_deref()) {
        return false;
    }

    #[cfg(unix)]
    if let Some(pgid) = record.pgid {
        if pgid <= 0 || pgid as u32 != record.root_pid {
            return false;
        }
        cleanup_process_group(pgid as libc::pid_t);
        return true;
    }

    false
}

fn process_identity_matches(pid: u32, started_at: Option<&str>) -> bool {
    if !crate::core::daemon::pid_is_running(pid) {
        return false;
    }
    match (started_at, process_started_at(pid)) {
        (Some(expected), Some(actual)) => expected == actual,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn process_started_at(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let started = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!started.is_empty()).then_some(started)
}

#[cfg(unix)]
fn cleanup_process_group(pgid: libc::pid_t) {
    unsafe {
        libc::kill(-pgid, libc::SIGTERM);
    }
    std::thread::sleep(CHILD_CLEANUP_GRACE);
    unsafe {
        if libc::kill(-pgid, 0) == 0 {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

struct InvocationIndexLock {
    path: PathBuf,
}

impl InvocationIndexLock {
    fn acquire() -> Result<Self> {
        let dir = invocation_leases_dir()?;
        fs::create_dir_all(&dir).map_err(|e| {
            Error::internal_unexpected(format!(
                "Failed to create invocation lease directory: {}",
                e
            ))
        })?;
        let path = dir.join(LOCK_NAME);
        for _ in 0..LOCK_ATTEMPTS {
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    remove_stale_index_lock(&path)?;
                    thread::sleep(LOCK_SLEEP);
                }
                Err(e) => {
                    return Err(Error::internal_unexpected(format!(
                        "Failed to acquire invocation lease lock {}: {}",
                        path.display(),
                        e
                    )))
                }
            }
        }
        Err(Error::internal_unexpected(format!(
            "Timed out acquiring invocation lease lock {}",
            path.display()
        )))
    }
}

impl Drop for InvocationIndexLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.path);
    }
}

fn remove_stale_index_lock(path: &Path) -> Result<()> {
    let Ok(metadata) = fs::metadata(path) else {
        return Ok(());
    };
    let Ok(modified) = metadata.modified() else {
        return Ok(());
    };
    if SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age > LOCK_STALE_AFTER)
    {
        fs::remove_dir(path).map_err(|e| {
            Error::internal_unexpected(format!(
                "Failed to remove stale invocation lease lock {}: {}",
                path.display(),
                e
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/core/engine/invocation_test.rs"]
mod invocation_test;

#[cfg(test)]
mod audit_coverage_tests {
    use super::*;
    use crate::engine::run_dir::RunDir;
    use crate::test_support::with_isolated_home;

    #[test]
    fn test_env_vars() {
        with_isolated_home(|_| {
            let run_dir = RunDir::create().expect("run dir");
            let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
                .expect("invocation guard");
            let env = guard.env_vars();

            assert!(env.iter().any(|(key, _)| key == "HOMEBOY_INVOCATION_ID"));
            assert!(env
                .iter()
                .any(|(key, _)| key == "HOMEBOY_INVOCATION_TMP_DIR"));
        });
    }
}
