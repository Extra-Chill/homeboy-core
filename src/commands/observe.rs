use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use clap::Args;
use regex::Regex;
use serde::Serialize;

use homeboy::component;
use homeboy::engine::run_dir::{self, RunDir};
use homeboy::extension::trace::{TraceArtifact, TraceEvent, TraceResults, TraceStatus};
use homeboy::git::short_head_revision_at;
use homeboy::observation::{NewRunRecord, ObservationStore, RunStatus};
use homeboy::Error;

use super::{CmdResult, GlobalArgs};

const DEFAULT_DURATION: &str = "30s";
const POLL_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Args, Clone)]
pub struct ObserveArgs {
    /// Component ID whose live system is being observed.
    pub component: String,

    /// Observation duration, e.g. 30s, 5m, 1h.
    #[arg(long, default_value = DEFAULT_DURATION, value_parser = parse_duration)]
    pub duration: Duration,

    /// Log file to tail. Repeatable.
    #[arg(long = "tail-log", value_name = "PATH")]
    pub tail_logs: Vec<PathBuf>,

    /// Only emit tailed log lines matching this regex. Applies to all --tail-log probes.
    #[arg(long, value_name = "REGEX")]
    pub grep: Option<String>,

    /// Regex matched against process command lines during snapshots. Repeatable.
    #[arg(long = "watch-process", value_name = "REGEX")]
    pub watch_processes: Vec<String>,
}

#[derive(Serialize)]
pub struct ObserveOutput {
    pub command: &'static str,
    pub run_id: String,
    pub component_id: String,
    pub status: String,
    pub duration_ms: u64,
    pub event_count: usize,
    pub artifact_path: String,
    pub hints: Vec<String>,
}

struct TailLogState {
    path: PathBuf,
    offset: u64,
    grep: Option<Regex>,
}

struct ProcessWatchState {
    pattern: String,
    regex: Regex,
    seen: BTreeSet<u32>,
}

pub fn run(args: ObserveArgs, _global: &GlobalArgs) -> CmdResult<ObserveOutput> {
    validate_probe_selection(&args)?;
    let comp = component::load(&args.component)?;
    let component_path = PathBuf::from(&comp.local_path);
    let run_dir = RunDir::create()?;
    let trace_path = run_dir.step_file(run_dir::files::TRACE_RESULTS);
    let store = ObservationStore::open_initialized()?;
    let command = observe_command(&args);
    let initial_metadata = serde_json::json!({
        "duration_ms": duration_millis(args.duration),
        "tail_logs": args.tail_logs,
        "grep": args.grep,
        "watch_processes": args.watch_processes,
        "run_dir": run_dir.path(),
    });

    let run = store.start_run(NewRunRecord {
        kind: "observe".to_string(),
        component_id: Some(comp.id.clone()),
        command: Some(command),
        cwd: Some(component_path.to_string_lossy().to_string()),
        homeboy_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        git_sha: short_head_revision_at(&component_path),
        rig_id: None,
        metadata_json: initial_metadata,
    })?;

    let mut status = RunStatus::Pass;
    let observe_result = collect_timeline(&args);
    let (timeline, failure) = match observe_result {
        Ok(timeline) => (timeline, None),
        Err(error) => {
            status = RunStatus::Error;
            (
                vec![event(
                    0,
                    "observe",
                    "error",
                    [("message", error.to_string())],
                )],
                Some(error.to_string()),
            )
        }
    };

    let results = TraceResults {
        component_id: comp.id.clone(),
        scenario_id: "observe".to_string(),
        status: trace_status_for_run_status(status),
        summary: Some("Passive observation timeline".to_string()),
        failure: failure.clone(),
        rig: None,
        timeline,
        span_definitions: Vec::new(),
        span_results: Vec::new(),
        assertions: Vec::new(),
        artifacts: vec![TraceArtifact {
            label: "observe timeline".to_string(),
            path: trace_path.to_string_lossy().to_string(),
        }],
    };

    write_trace_results(&trace_path, &results)?;
    let _ = store.record_artifact(&run.id, "trace-results", &trace_path);
    let finished = store.finish_run(
        &run.id,
        status,
        Some(serde_json::json!({
            "duration_ms": duration_millis(args.duration),
            "event_count": results.timeline.len(),
            "trace_results_path": trace_path,
            "failure": failure,
        })),
    )?;

    Ok((
        ObserveOutput {
            command: "observe",
            run_id: finished.id.clone(),
            component_id: comp.id,
            status: finished.status,
            duration_ms: duration_millis(args.duration),
            event_count: results.timeline.len(),
            artifact_path: trace_path.to_string_lossy().to_string(),
            hints: vec![
                format!("View this run: homeboy runs show {}", finished.id),
                "List observe runs: homeboy runs list --kind observe".to_string(),
            ],
        },
        if status == RunStatus::Pass { 0 } else { 1 },
    ))
}

fn validate_probe_selection(args: &ObserveArgs) -> homeboy::Result<()> {
    if args.tail_logs.is_empty() && args.watch_processes.is_empty() {
        return Err(Error::validation_invalid_argument(
            "probe",
            "observe requires at least one --tail-log or --watch-process probe",
            None,
            Some(vec![
                "homeboy observe my-component --duration 30s --tail-log /path/to/app.log"
                    .to_string(),
                "homeboy observe my-component --duration 30s --watch-process 'node .*serve'"
                    .to_string(),
            ]),
        ));
    }
    Ok(())
}

fn collect_timeline(args: &ObserveArgs) -> homeboy::Result<Vec<TraceEvent>> {
    let start = Instant::now();
    let mut tail_logs = build_tail_log_states(args)?;
    let mut process_watches = build_process_watch_states(args)?;
    let mut timeline = vec![empty_event(0, "observe", "started")];

    loop {
        let t_ms = elapsed_ms(start);
        for tail in &mut tail_logs {
            timeline.extend(poll_tail_log(tail, t_ms)?);
        }
        for watch in &mut process_watches {
            timeline.extend(poll_process_watch(watch, t_ms)?);
        }

        if start.elapsed() >= args.duration {
            break;
        }
        thread::sleep(POLL_INTERVAL.min(args.duration.saturating_sub(start.elapsed())));
    }

    timeline.push(empty_event(elapsed_ms(start), "observe", "finished"));
    Ok(timeline)
}

fn build_tail_log_states(args: &ObserveArgs) -> homeboy::Result<Vec<TailLogState>> {
    let grep = args
        .grep
        .as_deref()
        .map(|pattern| Regex::new(pattern).map_err(|e| invalid_regex("grep", pattern, e)))
        .transpose()?;

    args.tail_logs
        .iter()
        .map(|path| {
            let offset = File::open(path)
                .and_then(|file| file.metadata())
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            Ok(TailLogState {
                path: path.clone(),
                offset,
                grep: grep.clone(),
            })
        })
        .collect()
}

fn build_process_watch_states(args: &ObserveArgs) -> homeboy::Result<Vec<ProcessWatchState>> {
    args.watch_processes
        .iter()
        .map(|pattern| {
            Ok(ProcessWatchState {
                pattern: pattern.clone(),
                regex: Regex::new(pattern)
                    .map_err(|e| invalid_regex("watch-process", pattern, e))?,
                seen: BTreeSet::new(),
            })
        })
        .collect()
}

fn poll_tail_log(state: &mut TailLogState, t_ms: u64) -> homeboy::Result<Vec<TraceEvent>> {
    let mut file = match File::open(&state.path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(Error::internal_io(
                format!("Failed to open log {}: {}", state.path.display(), error),
                Some("observe.tail_log.open".to_string()),
            ))
        }
    };

    let len = file
        .metadata()
        .map_err(|e| {
            Error::internal_io(e.to_string(), Some("observe.tail_log.metadata".to_string()))
        })?
        .len();
    if len < state.offset {
        state.offset = 0;
    }
    if len == state.offset {
        return Ok(Vec::new());
    }

    file.seek(SeekFrom::Start(state.offset)).map_err(|e| {
        Error::internal_io(e.to_string(), Some("observe.tail_log.seek".to_string()))
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content).map_err(|e| {
        Error::internal_io(e.to_string(), Some("observe.tail_log.read".to_string()))
    })?;
    state.offset = len;

    Ok(content
        .lines()
        .filter(|line| {
            state
                .grep
                .as_ref()
                .map(|re| re.is_match(line))
                .unwrap_or(true)
        })
        .map(|line| {
            event(
                t_ms,
                "log",
                "line",
                [
                    ("path", state.path.to_string_lossy().to_string()),
                    ("line", line.to_string()),
                ],
            )
        })
        .collect())
}

fn poll_process_watch(
    state: &mut ProcessWatchState,
    t_ms: u64,
) -> homeboy::Result<Vec<TraceEvent>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output()
        .map_err(|e| Error::internal_io(e.to_string(), Some("observe.process.ps".to_string())))?;
    if !output.status.success() {
        return Err(Error::internal_unexpected(format!(
            "ps failed while observing processes: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current = BTreeSet::new();
    let mut events = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        let Some((pid_raw, command)) = trimmed.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(pid) = pid_raw.trim().parse::<u32>() else {
            continue;
        };
        let command = command.trim();
        if state.regex.is_match(command) {
            current.insert(pid);
            if state.seen.insert(pid) {
                events.push(event(
                    t_ms,
                    "process",
                    "matched",
                    [
                        ("pattern", state.pattern.clone()),
                        ("pid", pid.to_string()),
                        ("command", command.to_string()),
                    ],
                ));
            }
        }
    }

    let stopped: Vec<u32> = state.seen.difference(&current).copied().collect();
    for pid in stopped {
        state.seen.remove(&pid);
        events.push(event(
            t_ms,
            "process",
            "exited",
            [("pattern", state.pattern.clone()), ("pid", pid.to_string())],
        ));
    }

    Ok(events)
}

fn write_trace_results(path: &Path, results: &TraceResults) -> homeboy::Result<()> {
    let content = serde_json::to_string_pretty(results).map_err(|e| {
        Error::internal_unexpected(format!("Failed to serialize observe timeline: {e}"))
    })?;
    std::fs::write(path, content).map_err(|e| {
        Error::internal_io(
            format!("Failed to write observe timeline {}: {}", path.display(), e),
            Some("observe.write_trace_results".to_string()),
        )
    })
}

fn trace_status_for_run_status(status: RunStatus) -> TraceStatus {
    match status {
        RunStatus::Pass => TraceStatus::Pass,
        RunStatus::Fail => TraceStatus::Fail,
        _ => TraceStatus::Error,
    }
}

fn observe_command(args: &ObserveArgs) -> String {
    let mut parts = vec![
        "homeboy".to_string(),
        "observe".to_string(),
        args.component.clone(),
        "--duration".to_string(),
        format_duration(args.duration),
    ];
    for path in &args.tail_logs {
        parts.push("--tail-log".to_string());
        parts.push(path.to_string_lossy().to_string());
    }
    if let Some(grep) = &args.grep {
        parts.push("--grep".to_string());
        parts.push(grep.clone());
    }
    for pattern in &args.watch_processes {
        parts.push("--watch-process".to_string());
        parts.push(pattern.clone());
    }
    parts.join(" ")
}

fn event<K, V, I>(t_ms: u64, source: &str, name: &str, data: I) -> TraceEvent
where
    K: Into<String>,
    V: Into<serde_json::Value>,
    I: IntoIterator<Item = (K, V)>,
{
    TraceEvent {
        t_ms,
        source: source.to_string(),
        event: name.to_string(),
        data: data
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn empty_event(t_ms: u64, source: &str, name: &str) -> TraceEvent {
    TraceEvent {
        t_ms,
        source: source.to_string(),
        event: name.to_string(),
        data: BTreeMap::new(),
    }
}

fn parse_duration(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let split = raw
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(|| "expected duration like 500ms, 30s, 5m, or 1h".to_string())?;
    let (amount_raw, unit) = raw.split_at(split);
    let amount = amount_raw
        .parse::<u64>()
        .map_err(|_| "duration amount must be a positive integer".to_string())?;
    if amount == 0 {
        return Err("duration amount must be greater than zero".to_string());
    }

    match unit {
        "ms" => Ok(Duration::from_millis(amount)),
        "s" => Ok(Duration::from_secs(amount)),
        "m" => Ok(Duration::from_secs(amount * 60)),
        "h" => Ok(Duration::from_secs(amount * 60 * 60)),
        _ => Err("duration unit must be one of ms, s, m, or h".to_string()),
    }
}

fn format_duration(duration: Duration) -> String {
    if duration.as_millis() < 1000 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}s", duration.as_secs())
    }
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

fn elapsed_ms(start: Instant) -> u64 {
    duration_millis(start.elapsed())
}

fn invalid_regex(field: &str, pattern: &str, error: regex::Error) -> Error {
    Error::validation_invalid_argument(
        field,
        format!("invalid regex `{pattern}`: {error}"),
        Some(pattern.to_string()),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_accepts_supported_units() {
        assert_eq!(parse_duration("250ms").unwrap(), Duration::from_millis(250));
        assert_eq!(parse_duration("2s").unwrap(), Duration::from_secs(2));
        assert_eq!(parse_duration("3m").unwrap(), Duration::from_secs(180));
        assert_eq!(parse_duration("1h").unwrap(), Duration::from_secs(3600));
    }

    #[test]
    fn tail_log_emits_matching_new_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        std::fs::write(&path, "already here\n").unwrap();
        let mut state = TailLogState {
            path: path.clone(),
            offset: std::fs::metadata(&path).unwrap().len(),
            grep: Some(Regex::new("invalid_grant").unwrap()),
        };

        std::fs::write(
            &path,
            "already here\nignore me\nHTTP 400 invalid_grant from oauth\n",
        )
        .unwrap();
        let events = poll_tail_log(&mut state, 42).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].t_ms, 42);
        assert_eq!(events[0].source, "log");
        assert_eq!(events[0].event, "line");
        assert_eq!(
            events[0].data.get("line").and_then(|value| value.as_str()),
            Some("HTTP 400 invalid_grant from oauth")
        );
    }
}
