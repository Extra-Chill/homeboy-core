//! Conservative AI provider/auth failure classification for bench runs.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::parsing::BenchResults;

const MAX_ARTIFACT_BYTES: u64 = 256 * 1024;
const MAX_EXCERPT_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum BenchProviderFailureClass {
    MissingApiKey,
    Auth,
    StreamTruncation,
    GatewayTimeout,
    RateLimit,
    ConcurrencyLimit,
}

impl BenchProviderFailureClass {
    pub fn as_str(self) -> &'static str {
        match self {
            BenchProviderFailureClass::MissingApiKey => "missing_api_key",
            BenchProviderFailureClass::Auth => "auth",
            BenchProviderFailureClass::StreamTruncation => "stream_truncation",
            BenchProviderFailureClass::GatewayTimeout => "gateway_timeout",
            BenchProviderFailureClass::RateLimit => "rate_limit",
            BenchProviderFailureClass::ConcurrencyLimit => "concurrency_limit",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BenchProviderFailure {
    pub class: BenchProviderFailureClass,
    pub reason: String,
    pub source: BenchProviderFailureSource,
    pub excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum BenchProviderFailureSource {
    Stderr,
    Artifact {
        scenario_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        run_index: Option<usize>,
        name: String,
        path: String,
    },
}

pub fn classify_text(text: &str) -> Option<(BenchProviderFailureClass, &'static str)> {
    let normalized = text.to_ascii_lowercase();

    if contains_any(
        &normalized,
        &[
            "no api key available",
            "missing api key",
            "api key is missing",
            "no api key provided",
            "openai_api_key is not set",
            "anthropic_api_key is not set",
            "google_api_key is not set",
            "gemini_api_key is not set",
        ],
    ) {
        return Some((BenchProviderFailureClass::MissingApiKey, "missing API key"));
    }

    if contains_any(
        &normalized,
        &[
            "concurrency limit",
            "concurrent request limit",
            "too many concurrent requests",
            "too many simultaneous requests",
        ],
    ) {
        return Some((
            BenchProviderFailureClass::ConcurrencyLimit,
            "provider concurrency limit",
        ));
    }

    if contains_any(
        &normalized,
        &[
            "rate limit",
            "rate_limit",
            "too many requests",
            "http 429",
            "status 429",
            "429 too many requests",
        ],
    ) {
        return Some((BenchProviderFailureClass::RateLimit, "provider rate limit"));
    }

    if contains_any(
        &normalized,
        &[
            "gateway timeout",
            "504 gateway",
            "http 504",
            "status 504",
            "upstream request timeout",
            "upstream timed out",
        ],
    ) {
        return Some((
            BenchProviderFailureClass::GatewayTimeout,
            "provider gateway timeout",
        ));
    }

    if contains_any(
        &normalized,
        &[
            "stream truncated",
            "truncated stream",
            "response stream ended early",
            "stream ended unexpectedly",
            "premature close",
        ],
    ) {
        return Some((
            BenchProviderFailureClass::StreamTruncation,
            "provider stream truncation",
        ));
    }

    if contains_any(
        &normalized,
        &[
            "auth error",
            "authentication failed",
            "invalid api key",
            "unauthorized api key",
            "401 unauthorized",
        ],
    ) {
        return Some((BenchProviderFailureClass::Auth, "provider auth failure"));
    }

    None
}

pub fn collect_provider_failures(
    results: Option<&BenchResults>,
    stderr_tail: Option<&str>,
    run_dir: &Path,
) -> Vec<BenchProviderFailure> {
    let mut failures = Vec::new();

    if let Some(stderr_tail) = stderr_tail {
        if let Some((class, reason)) = classify_text(stderr_tail) {
            failures.push(BenchProviderFailure {
                class,
                reason: reason.to_string(),
                source: BenchProviderFailureSource::Stderr,
                excerpt: excerpt(stderr_tail),
            });
        }
    }

    if let Some(results) = results {
        for scenario in &results.scenarios {
            for (name, artifact) in &scenario.artifacts {
                collect_artifact_failure(
                    &mut failures,
                    run_dir,
                    &scenario.id,
                    None,
                    name,
                    &artifact.path,
                );
            }
            if let Some(runs) = &scenario.runs {
                for (run_index, run) in runs.iter().enumerate() {
                    for (name, artifact) in &run.artifacts {
                        collect_artifact_failure(
                            &mut failures,
                            run_dir,
                            &scenario.id,
                            Some(run_index),
                            name,
                            &artifact.path,
                        );
                    }
                }
            }
        }
    }

    failures
}

fn collect_artifact_failure(
    failures: &mut Vec<BenchProviderFailure>,
    run_dir: &Path,
    scenario_id: &str,
    run_index: Option<usize>,
    name: &str,
    artifact_path: &str,
) {
    let Some(text) = read_artifact_text(run_dir, artifact_path) else {
        return;
    };
    let Some((class, reason)) = classify_text(&text) else {
        return;
    };

    failures.push(BenchProviderFailure {
        class,
        reason: reason.to_string(),
        source: BenchProviderFailureSource::Artifact {
            scenario_id: scenario_id.to_string(),
            run_index,
            name: name.to_string(),
            path: artifact_path.to_string(),
        },
        excerpt: excerpt(&text),
    });
}

fn read_artifact_text(run_dir: &Path, artifact_path: &str) -> Option<String> {
    let path = resolve_artifact_path(run_dir, artifact_path);
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_ARTIFACT_BYTES).read_to_end(&mut bytes).ok()?;
    String::from_utf8(bytes).ok()
}

fn resolve_artifact_path(run_dir: &Path, artifact_path: &str) -> PathBuf {
    let path = PathBuf::from(artifact_path);
    if path.is_absolute() {
        path
    } else {
        run_dir.join(path)
    }
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn excerpt(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= MAX_EXCERPT_CHARS {
        return compact;
    }

    let mut clipped = compact
        .chars()
        .take(MAX_EXCERPT_CHARS.saturating_sub(1))
        .collect::<String>();
    clipped.push_str("...");
    clipped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension::bench::artifact::BenchArtifact;
    use crate::extension::bench::parsing::{BenchMetrics, BenchScenario};
    use std::collections::BTreeMap;

    #[test]
    fn classifies_representative_provider_failures() {
        let cases = [
            (
                "Auth error: No API key available",
                BenchProviderFailureClass::MissingApiKey,
            ),
            (
                "OpenAI stream truncated before final chunk",
                BenchProviderFailureClass::StreamTruncation,
            ),
            (
                "provider returned 504 Gateway Timeout",
                BenchProviderFailureClass::GatewayTimeout,
            ),
            (
                "HTTP 429: too many requests",
                BenchProviderFailureClass::RateLimit,
            ),
            (
                "Too many concurrent requests for this model",
                BenchProviderFailureClass::ConcurrencyLimit,
            ),
            (
                "Authentication failed for provider account",
                BenchProviderFailureClass::Auth,
            ),
        ];

        for (text, class) in cases {
            assert_eq!(classify_text(text).map(|(class, _)| class), Some(class));
        }
    }

    #[test]
    fn does_not_classify_workload_assertion_failures() {
        assert_eq!(
            classify_text("assertion failed: expected title to be visible"),
            None
        );
        assert_eq!(classify_text("scenario gate failed: p95_ms lte 1000"), None);
        assert_eq!(
            classify_text("test failed because selector .submit was missing"),
            None
        );
    }

    #[test]
    fn scans_stderr_and_artifact_contents() {
        let run_dir = tempfile::TempDir::new().expect("run dir");
        let artifact_dir = run_dir.path().join("bench-artifacts/agent");
        std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
        std::fs::write(
            artifact_dir.join("transcript.txt"),
            "provider failed with 429 too many requests",
        )
        .expect("write artifact");

        let mut artifacts = BTreeMap::new();
        artifacts.insert(
            "transcript".to_string(),
            BenchArtifact {
                path: "bench-artifacts/agent/transcript.txt".to_string(),
                kind: Some("text".to_string()),
                label: None,
            },
        );
        let results = BenchResults {
            component_id: "studio".to_string(),
            iterations: 1,
            run_metadata: None,
            scenarios: vec![BenchScenario {
                id: "agent".to_string(),
                file: None,
                source: None,
                default_iterations: None,
                tags: Vec::new(),
                iterations: 1,
                metrics: BenchMetrics::default(),
                metric_groups: BTreeMap::new(),
                gates: Vec::new(),
                gate_results: Vec::new(),
                passed: true,
                memory: None,
                artifacts,
                runs: None,
                runs_summary: None,
            }],
            metric_policies: BTreeMap::new(),
        };

        let failures = collect_provider_failures(
            Some(&results),
            Some("Auth error: No API key available"),
            run_dir.path(),
        );

        assert_eq!(failures.len(), 2);
        assert_eq!(failures[0].class, BenchProviderFailureClass::MissingApiKey);
        assert_eq!(failures[1].class, BenchProviderFailureClass::RateLimit);
    }
}
