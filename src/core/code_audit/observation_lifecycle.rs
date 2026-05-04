//! Repeated observation lifecycle orchestration detector.
//!
//! This detector is intentionally runtime-agnostic. It does not know about
//! `bench`, `trace`, `review`, or any concrete observation type. Instead it looks
//! for multiple source files that each own the same observation lifecycle phases:
//! start, metadata attachment, success/failure finalization, artifact/export, and
//! cleanup/error handling. Tiny call sites that only delegate to a wrapper do not
//! carry enough phases to match.

use std::collections::HashMap;

use super::conventions::AuditFinding;
use super::findings::{Finding, Severity};
use super::fingerprint::FileFingerprint;

const MIN_PHASES: usize = 3;
const MIN_FILES_PER_SHAPE: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LifecyclePhase {
    Start,
    Metadata,
    Finalize,
    Artifact,
    Failure,
}

impl LifecyclePhase {
    fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Metadata => "metadata",
            Self::Finalize => "finalize",
            Self::Artifact => "artifact",
            Self::Failure => "failure",
        }
    }
}

#[derive(Debug)]
struct Candidate<'a> {
    fp: &'a FileFingerprint,
    phases: Vec<LifecyclePhase>,
    functions: Vec<String>,
}

pub(super) fn run(fingerprints: &[&FileFingerprint]) -> Vec<Finding> {
    let candidates: Vec<Candidate<'_>> = fingerprints
        .iter()
        .filter(|fp| !super::walker::is_test_path(&fp.relative_path))
        .filter_map(|fp| candidate_for(fp))
        .collect();

    let mut by_shape: HashMap<Vec<LifecyclePhase>, Vec<&Candidate<'_>>> = HashMap::new();
    for candidate in &candidates {
        by_shape
            .entry(candidate.phases.clone())
            .or_default()
            .push(candidate);
    }

    let mut findings = Vec::new();
    for (phases, members) in by_shape {
        if members.len() < MIN_FILES_PER_SHAPE {
            continue;
        }

        let mut files: Vec<String> = members
            .iter()
            .map(|candidate| candidate.fp.relative_path.clone())
            .collect();
        files.sort();

        let phase_list = phases
            .iter()
            .map(|phase| phase.label())
            .collect::<Vec<_>>()
            .join("/");
        let function_list = members
            .iter()
            .flat_map(|candidate| candidate.functions.iter())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let functions = if function_list.is_empty() {
            "<unknown>".to_string()
        } else {
            function_list.join(", ")
        };

        findings.push(Finding {
            convention: "observation_lifecycle".to_string(),
            severity: Severity::Warning,
            file: files[0].clone(),
            description: format!(
                "Repeated observation lifecycle scaffolding owns phases `{}` across {} file(s): {}. Functions: {}.",
                phase_list,
                files.len(),
                files.join(", "),
                functions
            ),
            suggestion: "Move observation lifecycle ownership into a shared lifecycle helper or descriptor-owned execution wrapper so commands only describe work and artifacts.".to_string(),
            kind: AuditFinding::ObservationLifecycleScaffolding,
        });
    }

    findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.description.cmp(&b.description)));
    findings
}

fn candidate_for(fp: &FileFingerprint) -> Option<Candidate<'_>> {
    let production_content = production_content(&fp.content);
    if !contains_observation(production_content) {
        return None;
    }

    let mut phases = Vec::new();
    for phase in [
        LifecyclePhase::Start,
        LifecyclePhase::Metadata,
        LifecyclePhase::Finalize,
        LifecyclePhase::Artifact,
        LifecyclePhase::Failure,
    ] {
        if content_has_phase(production_content, phase) {
            phases.push(phase);
        }
    }

    if phases.len() < MIN_PHASES {
        return None;
    }

    Some(Candidate {
        fp,
        phases,
        functions: lifecycle_functions(fp),
    })
}

fn production_content(content: &str) -> &str {
    content
        .find("#[cfg(test)]")
        .map(|offset| &content[..offset])
        .unwrap_or(content)
}

fn contains_observation(content: &str) -> bool {
    content.to_ascii_lowercase().contains("observation")
}

fn content_has_phase(content: &str, phase: LifecyclePhase) -> bool {
    content.lines().any(|line| line_has_phase(line, phase))
}

fn line_has_phase(line: &str, phase: LifecyclePhase) -> bool {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("observation") {
        return false;
    }

    phase_tokens(phase)
        .iter()
        .any(|token| contains_wordish(&lower, token))
}

fn phase_tokens(phase: LifecyclePhase) -> &'static [&'static str] {
    match phase {
        LifecyclePhase::Start => &["start", "begin", "create", "open", "acquire", "record"],
        LifecyclePhase::Metadata => &["metadata", "meta", "context", "attach"],
        LifecyclePhase::Finalize => &["finalize", "finish", "complete", "success", "close"],
        LifecyclePhase::Artifact => &["artifact", "export", "bundle", "report", "output"],
        LifecyclePhase::Failure => &["failure", "failed", "error", "err", "cleanup", "rollback"],
    }
}

fn contains_wordish(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .any(|part| {
            part == needle
                || part.starts_with(&format!("{}_", needle))
                || part.ends_with(&format!("_{}", needle))
        })
}

fn lifecycle_functions(fp: &FileFingerprint) -> Vec<String> {
    let mut names: Vec<String> = fp
        .methods
        .iter()
        .filter(|method| {
            let lower = method.to_ascii_lowercase();
            contains_wordish(&lower, "run")
                || contains_wordish(&lower, "execute")
                || contains_wordish(&lower, "handle")
                || contains_wordish(&lower, "observe")
                || contains_wordish(&lower, "observation")
        })
        .cloned()
        .collect();
    if names.is_empty() && !fp.methods.is_empty() {
        names.extend(fp.methods.iter().take(3).cloned());
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_audit::conventions::Language;

    fn rust_fp(path: &str, methods: &[&str], content: &str) -> FileFingerprint {
        FileFingerprint {
            relative_path: path.to_string(),
            language: Language::Rust,
            methods: methods.iter().map(|method| method.to_string()).collect(),
            content: content.to_string(),
            ..Default::default()
        }
    }

    fn duplicated_lifecycle_body(name: &str) -> String {
        format!(
            r#"
fn {name}() {{
    let observation = start_observation_run();
    observation.attach_metadata("component", "demo");
    let result = do_work();
    observation.write_artifact("report.json");
    if result.is_ok() {{ observation.finalize_success(); }}
    if result.is_err() {{ observation.record_failure(); }}
}}
"#
        )
    }

    #[test]
    fn flags_repeated_observation_lifecycle_scaffolding() {
        let bench = rust_fp(
            "src/commands/bench.rs",
            &["run_bench"],
            &duplicated_lifecycle_body("run_bench"),
        );
        let trace = rust_fp(
            "src/commands/trace.rs",
            &["run_trace"],
            &duplicated_lifecycle_body("run_trace"),
        );

        let findings = run(&[&bench, &trace]);

        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].kind,
            AuditFinding::ObservationLifecycleScaffolding
        );
        assert!(findings[0]
            .description
            .contains("start/metadata/finalize/artifact/failure"));
        assert!(findings[0].description.contains("src/commands/bench.rs"));
        assert!(findings[0].suggestion.contains("shared lifecycle helper"));
    }

    #[test]
    fn ignores_tiny_shared_helper_call_sites() {
        let bench = rust_fp(
            "src/commands/bench.rs",
            &["run_bench"],
            r#"fn run_bench() { observation_runner.execute(descriptor, || do_work()); }"#,
        );
        let trace = rust_fp(
            "src/commands/trace.rs",
            &["run_trace"],
            r#"fn run_trace() { observation_runner.execute(descriptor, || do_work()); }"#,
        );

        assert!(run(&[&bench, &trace]).is_empty());
    }

    #[test]
    fn ignores_single_file_lifecycle_owner() {
        let helper = rust_fp(
            "src/core/observation_lifecycle.rs",
            &["execute_with_observation"],
            &duplicated_lifecycle_body("execute_with_observation"),
        );

        assert!(run(&[&helper]).is_empty());
    }

    #[test]
    fn ignores_inline_test_fixture_strings() {
        let detector = rust_fp(
            "src/core/code_audit/observation_lifecycle.rs",
            &["run"],
            r##"
pub(super) fn run() {}

#[cfg(test)]
mod tests {
    const FIXTURE: &str = r#"
        let observation = start_observation_run();
        observation.attach_metadata("component", "demo");
        observation.write_artifact("report.json");
        observation.finalize_success();
        observation.record_failure();
    "#;
}
"##,
        );

        assert!(run(&[&detector]).is_empty());
    }
}
