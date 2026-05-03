use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::engine::run_dir::RunDir;
use crate::error::{Error, Result};
use crate::http_probe;

use super::parsing::{TraceArtifact, TraceEvent, TraceResults};

const TRACE_ATTACHMENTS_ARTIFACT: &str = "trace-attachments.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceAttachment {
    pub kind: String,
    pub target: String,
}

impl TraceAttachment {
    pub fn parse_all(raw_attachments: &[String]) -> Result<Vec<Self>> {
        raw_attachments
            .iter()
            .map(|attachment| Self::parse(attachment))
            .collect()
    }

    pub fn parse(raw: &str) -> Result<Self> {
        if raw.starts_with("http://") || raw.starts_with("https://") {
            return Ok(Self {
                kind: "http".to_string(),
                target: raw.to_string(),
            });
        }
        let Some((kind, target)) = raw.split_once(':') else {
            return Err(invalid_attachment(raw, "expected KIND:TARGET"));
        };
        if target.is_empty() {
            return Err(invalid_attachment(raw, "attachment target cannot be empty"));
        }
        match kind {
            "logfile" => Ok(Self {
                kind: kind.to_string(),
                target: target.to_string(),
            }),
            "pid" => {
                let pid = target.parse::<u32>().map_err(|_| {
                    invalid_attachment(raw, "pid target must be a positive integer")
                })?;
                if pid == 0 {
                    return Err(invalid_attachment(
                        raw,
                        "pid target must be greater than zero",
                    ));
                }
                Ok(Self {
                    kind: kind.to_string(),
                    target: target.to_string(),
                })
            }
            "port" => {
                let port = target
                    .parse::<u16>()
                    .map_err(|_| invalid_attachment(raw, "port target must be a TCP port"))?;
                if port == 0 {
                    return Err(invalid_attachment(
                        raw,
                        "port target must be greater than zero",
                    ));
                }
                Ok(Self {
                    kind: kind.to_string(),
                    target: port.to_string(),
                })
            }
            "http" => {
                if !(target.starts_with("http://") || target.starts_with("https://")) {
                    return Err(invalid_attachment(
                        raw,
                        "http target must start with http:// or https://",
                    ));
                }
                Ok(Self {
                    kind: kind.to_string(),
                    target: target.to_string(),
                })
            }
            _ => Err(invalid_attachment(
                raw,
                "supported attachment kinds are logfile, pid, port, and http",
            )),
        }
    }
}

fn invalid_attachment(raw: &str, reason: &str) -> Error {
    Error::validation_invalid_argument(
        "trace.attach",
        format!("invalid trace attachment `{raw}`: {reason}"),
        None,
        None,
    )
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct TraceAttachmentObservation {
    phase: &'static str,
    elapsed_ms: u64,
    attachment: TraceAttachment,
    status: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    data: BTreeMap<String, serde_json::Value>,
}

pub(super) fn observe_trace_attachments(
    attachments: &[TraceAttachment],
    phase: &'static str,
    started_at: Instant,
) -> Vec<TraceAttachmentObservation> {
    attachments
        .iter()
        .map(|attachment| {
            let (status, data) = observe_trace_attachment(attachment);
            TraceAttachmentObservation {
                phase,
                elapsed_ms: started_at
                    .elapsed()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX),
                attachment: attachment.clone(),
                status,
                data,
            }
        })
        .collect()
}

fn observe_trace_attachment(
    attachment: &TraceAttachment,
) -> (String, BTreeMap<String, serde_json::Value>) {
    match attachment.kind.as_str() {
        "logfile" => observe_logfile(&attachment.target),
        "pid" => observe_pid(&attachment.target),
        "port" => observe_port(&attachment.target),
        "http" => observe_http(&attachment.target),
        _ => ("unsupported".to_string(), BTreeMap::new()),
    }
}

fn observe_logfile(path: &str) -> (String, BTreeMap<String, serde_json::Value>) {
    let mut data = BTreeMap::new();
    data.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    match std::fs::metadata(path) {
        Ok(metadata) => {
            data.insert("bytes".to_string(), serde_json::json!(metadata.len()));
            ("present".to_string(), data)
        }
        Err(error) => {
            data.insert(
                "error".to_string(),
                serde_json::Value::String(error.to_string()),
            );
            ("missing".to_string(), data)
        }
    }
}

fn observe_pid(raw_pid: &str) -> (String, BTreeMap<String, serde_json::Value>) {
    let mut data = BTreeMap::new();
    data.insert("pid".to_string(), serde_json::json!(raw_pid));
    let Ok(pid) = raw_pid.parse::<u32>() else {
        return ("error".to_string(), data);
    };
    let running = process_exists(pid);
    data.insert("running".to_string(), serde_json::json!(running));
    if running {
        ("running".to_string(), data)
    } else {
        ("missing".to_string(), data)
    }
}

#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(not(unix))]
fn process_exists(_pid: u32) -> bool {
    false
}

fn observe_port(raw_port: &str) -> (String, BTreeMap<String, serde_json::Value>) {
    let mut data = BTreeMap::new();
    data.insert("port".to_string(), serde_json::json!(raw_port));
    let Ok(port) = raw_port.parse::<u16>() else {
        return ("error".to_string(), data);
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listening = TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok();
    data.insert("listening".to_string(), serde_json::json!(listening));
    if listening {
        ("listening".to_string(), data)
    } else {
        ("closed".to_string(), data)
    }
}

fn observe_http(url: &str) -> (String, BTreeMap<String, serde_json::Value>) {
    let mut data = BTreeMap::new();
    data.insert(
        "url".to_string(),
        serde_json::Value::String(url.to_string()),
    );
    match http_probe::get_status(url, Duration::from_secs(1)) {
        Ok(status) => {
            data.insert("status_code".to_string(), serde_json::json!(status));
            ("reachable".to_string(), data)
        }
        Err(error) => {
            data.insert(
                "error".to_string(),
                serde_json::Value::String(error.message),
            );
            ("unreachable".to_string(), data)
        }
    }
}

pub(super) fn append_attach_observations(
    results: &mut TraceResults,
    run_dir: &RunDir,
    observations: &[TraceAttachmentObservation],
) -> Result<()> {
    if observations.is_empty() {
        return Ok(());
    }

    for observation in observations {
        let mut data = observation.data.clone();
        data.insert(
            "kind".to_string(),
            serde_json::Value::String(observation.attachment.kind.clone()),
        );
        data.insert(
            "target".to_string(),
            serde_json::Value::String(observation.attachment.target.clone()),
        );
        data.insert(
            "status".to_string(),
            serde_json::Value::String(observation.status.clone()),
        );
        results.timeline.push(TraceEvent {
            t_ms: observation.elapsed_ms,
            source: format!("attach.{}", observation.attachment.kind),
            event: format!("{}.{}", observation.phase, observation.status),
            data,
        });
    }

    let artifact_path = run_dir
        .path()
        .join("artifacts")
        .join(TRACE_ATTACHMENTS_ARTIFACT);
    std::fs::write(
        &artifact_path,
        serde_json::to_string_pretty(observations).map_err(|e| {
            Error::internal_json(
                format!("Failed to serialize trace attachment observations: {e}"),
                Some("trace.attach.observations.serialize".to_string()),
            )
        })?,
    )
    .map_err(|e| {
        Error::internal_io(
            format!(
                "Failed to write trace attachment artifact {}: {}",
                artifact_path.display(),
                e
            ),
            Some("trace.attach.observations.write".to_string()),
        )
    })?;
    results.artifacts.push(TraceArtifact {
        label: "Trace attachments".to_string(),
        path: format!("artifacts/{TRACE_ATTACHMENTS_ARTIFACT}"),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_attachment_parse_supports_v1_kinds() {
        assert_eq!(
            TraceAttachment::parse("logfile:/tmp/service.log").unwrap(),
            TraceAttachment {
                kind: "logfile".to_string(),
                target: "/tmp/service.log".to_string(),
            }
        );
        assert_eq!(TraceAttachment::parse("pid:1234").unwrap().target, "1234");
        assert_eq!(TraceAttachment::parse("port:8080").unwrap().target, "8080");
        assert_eq!(
            TraceAttachment::parse("http:http://127.0.0.1:8080/health")
                .unwrap()
                .target,
            "http://127.0.0.1:8080/health"
        );
        assert_eq!(
            TraceAttachment::parse("http://127.0.0.1:8080/health")
                .unwrap()
                .target,
            "http://127.0.0.1:8080/health"
        );
        assert!(TraceAttachment::parse("systemd:kimaki.service").is_err());
    }
}
