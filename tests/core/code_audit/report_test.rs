use crate::code_audit::report::{build_audit_summary, build_fix_hints, build_fix_policy_summary};
use crate::code_audit::{AuditSummary, CodeAuditResult, Finding, Severity};
use crate::refactor::auto::PolicySummary;

fn empty_result() -> CodeAuditResult {
    CodeAuditResult {
        component_id: "test-component".to_string(),
        source_path: "/tmp/test".to_string(),
        summary: AuditSummary {
            files_scanned: 0,
            conventions_detected: 0,
            outliers_found: 0,
            alignment_score: None,
            files_skipped: 0,
            warnings: vec![],
        },
        conventions: vec![],
        directory_conventions: vec![],
        findings: vec![],
        duplicate_groups: vec![],
    }
}

fn make_finding(severity: Severity) -> Finding {
    Finding {
        convention: "TestConvention".to_string(),
        severity,
        file: "src/example.rs".to_string(),
        description: "Test finding".to_string(),
        suggestion: "Fix it".to_string(),
        kind: crate::code_audit::AuditFinding::MissingMethod,
    }
}

#[test]
fn test_build_audit_summary_empty_result() {
    let result = empty_result();
    let summary = build_audit_summary(&result, 0);

    assert_eq!(summary.total_findings, 0);
    assert_eq!(summary.warnings, 0);
    assert_eq!(summary.info, 0);
    assert_eq!(summary.exit_code, 0);
    assert!(summary.top_findings.is_empty());
    assert_eq!(summary.alignment_score, None);
}

#[test]
fn test_build_audit_summary_counts_severities() {
    let mut result = empty_result();
    result.findings.push(make_finding(Severity::Warning));
    result.findings.push(make_finding(Severity::Warning));
    result.findings.push(make_finding(Severity::Info));

    let summary = build_audit_summary(&result, 1);

    assert_eq!(summary.total_findings, 3);
    assert_eq!(summary.warnings, 2);
    assert_eq!(summary.info, 1);
    assert_eq!(summary.exit_code, 1);
}

#[test]
fn test_build_audit_summary_caps_top_findings_at_20() {
    let mut result = empty_result();
    for _ in 0..25 {
        result.findings.push(make_finding(Severity::Warning));
    }

    let summary = build_audit_summary(&result, 1);

    assert_eq!(summary.total_findings, 25);
    assert_eq!(summary.top_findings.len(), 20);
}

#[test]
fn test_build_audit_summary_preserves_alignment_score() {
    let mut result = empty_result();
    result.summary.alignment_score = Some(0.85);

    let summary = build_audit_summary(&result, 0);

    assert_eq!(summary.alignment_score, Some(0.85));
}

#[test]
fn test_build_fix_hints_empty_when_no_blocked() {
    let policy = PolicySummary::default();
    let hints = build_fix_hints(false, &policy);
    assert!(hints.is_empty());
}

#[test]
fn test_build_fix_hints_dry_run_with_blocked() {
    let policy = PolicySummary {
        blocked_insertions: 2,
        blocked_new_files: 1,
        ..Default::default()
    };
    let hints = build_fix_hints(false, &policy);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].contains("3 fix(es)"));
    assert!(hints[0].contains("blocked"));
}

#[test]
fn test_build_fix_hints_written_with_blocked() {
    let policy = PolicySummary {
        blocked_insertions: 1,
        blocked_new_files: 0,
        ..Default::default()
    };
    let hints = build_fix_hints(true, &policy);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].contains("Applied only safe_auto"));
}

#[test]
fn test_build_fix_hints_preflight_failures() {
    let policy = PolicySummary {
        preflight_failures: 3,
        ..Default::default()
    };
    let hints = build_fix_hints(false, &policy);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].contains("3 fix(es) failed deterministic preflight"));
}

#[test]
fn test_build_fix_hints_multiple_conditions() {
    let policy = PolicySummary {
        blocked_insertions: 1,
        blocked_new_files: 1,
        preflight_failures: 2,
        ..Default::default()
    };
    let hints = build_fix_hints(true, &policy);
    // Should have both blocked hint and preflight hint
    assert_eq!(hints.len(), 2);
}

#[test]
fn test_build_fix_policy_summary_maps_fields() {
    let policy = PolicySummary {
        visible_insertions: 10,
        visible_new_files: 5,
        auto_apply_insertions: 7,
        auto_apply_new_files: 3,
        blocked_insertions: 3,
        blocked_new_files: 2,
        preflight_failures: 1,
    };
    let summary = build_fix_policy_summary(
        &policy,
        vec!["kind_a".to_string()],
        vec!["kind_b".to_string()],
    );

    assert_eq!(summary.selected_only, vec!["kind_a"]);
    assert_eq!(summary.excluded, vec!["kind_b"]);
    assert_eq!(summary.visible_insertions, 10);
    assert_eq!(summary.visible_new_files, 5);
    assert_eq!(summary.auto_apply_insertions, 7);
    assert_eq!(summary.auto_apply_new_files, 3);
    assert_eq!(summary.blocked_insertions, 3);
    assert_eq!(summary.blocked_new_files, 2);
    assert_eq!(summary.preflight_failures, 1);
}
