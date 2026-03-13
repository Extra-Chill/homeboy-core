use crate::code_audit::run::default_audit_exit_code;
use crate::code_audit::{AuditSummary, CodeAuditResult, Finding, Severity};

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
fn test_default_exit_code_full_no_outliers() {
    let result = empty_result();
    assert_eq!(default_audit_exit_code(&result, false), 0);
}

#[test]
fn test_default_exit_code_full_with_outliers() {
    let mut result = empty_result();
    result.summary.outliers_found = 3;
    assert_eq!(default_audit_exit_code(&result, false), 1);
}

#[test]
fn test_default_exit_code_scoped_no_findings() {
    let result = empty_result();
    assert_eq!(default_audit_exit_code(&result, true), 0);
}

#[test]
fn test_default_exit_code_scoped_with_findings() {
    let mut result = empty_result();
    result.findings.push(make_finding(Severity::Warning));
    assert_eq!(default_audit_exit_code(&result, true), 1);
}

#[test]
fn test_default_exit_code_scoped_ignores_outliers() {
    // Scoped mode only cares about findings, not outliers
    let mut result = empty_result();
    result.summary.outliers_found = 5;
    assert_eq!(default_audit_exit_code(&result, true), 0);
}

#[test]
fn test_default_exit_code_full_ignores_findings_without_outliers() {
    // Full mode uses outliers as the gate, not findings
    let mut result = empty_result();
    result.findings.push(make_finding(Severity::Info));
    assert_eq!(default_audit_exit_code(&result, false), 0);
}
