//! Code audit system for convention detection and drift analysis.
//!
//! Scans source code to discover structural conventions, detect outliers,
//! and report architectural drift. Works by:
//!
//! 1. Fingerprinting source files (extract methods, registrations, types)
//! 2. Grouping files by directory and language
//! 3. Discovering conventions (patterns most files follow)
//! 4. Checking all files against discovered conventions
//! 5. Producing actionable findings for outliers

mod checks;
mod conventions;
mod findings;
pub mod fixer;

use std::path::Path;

pub use checks::{CheckResult, CheckStatus};
pub use conventions::{Convention, Deviation, DeviationKind, Language, Outlier};
pub use findings::{Finding, Severity};

use crate::{component, Result};

/// Helper for `skip_serializing_if` on zero-value usize fields.
fn is_zero(v: &usize) -> bool {
    *v == 0
}

/// Summary counts for the audit report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditSummary {
    pub files_scanned: usize,
    pub conventions_detected: usize,
    #[serde(skip_serializing_if = "is_zero")]
    pub outliers_found: usize,
    /// Overall alignment score (0.0 = total chaos, 1.0 = perfect consistency).
    pub alignment_score: f32,
}

/// Complete result of auditing a component's code conventions.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeAuditResult {
    pub component_id: String,
    pub source_path: String,
    pub summary: AuditSummary,
    pub conventions: Vec<ConventionReport>,
    pub findings: Vec<Finding>,
}

/// A convention as reported to the user (includes check status).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ConventionReport {
    pub name: String,
    pub glob: String,
    pub status: CheckStatus,
    pub expected_methods: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expected_registrations: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub expected_interfaces: Vec<String>,
    pub conforming: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outliers: Vec<Outlier>,
    pub total_files: usize,
    pub confidence: f32,
}

// ============================================================================
// Public API
// ============================================================================

/// Audit a registered component by ID.
pub fn audit_component(component_id: &str) -> Result<CodeAuditResult> {
    let comp = component::load(component_id)?;
    component::validate_local_path(&comp)?;
    audit_path_with_id(component_id, &comp.local_path)
}

/// Audit a filesystem path directly (no registered component needed).
pub fn audit_path(path: &str) -> Result<CodeAuditResult> {
    let p = Path::new(path);
    if !p.is_dir() {
        return Err(crate::Error::validation_invalid_argument(
            "path",
            format!("Not a directory: {}", path),
            None,
            None,
        ));
    }

    // Use directory name as component_id
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    audit_path_with_id(&name, path)
}

/// Core audit logic shared by both entry points.
fn audit_path_with_id(component_id: &str, source_path: &str) -> Result<CodeAuditResult> {
    let root = Path::new(source_path);

    log_status!("audit", "Scanning {} for conventions...", source_path);

    // Phase 1: Auto-discover file groups
    let groups = conventions::auto_discover_groups(root);

    if groups.is_empty() {
        log_status!("audit", "No source files found");
        return Ok(CodeAuditResult {
            component_id: component_id.to_string(),
            source_path: source_path.to_string(),
            summary: AuditSummary {
                files_scanned: 0,
                conventions_detected: 0,
                outliers_found: 0,
                alignment_score: 1.0,
            },
            conventions: vec![],
            findings: vec![],
        });
    }

    // Phase 2: Discover conventions for each group
    let mut discovered_conventions = Vec::new();
    let mut total_files = 0;

    for (name, glob, fingerprints) in &groups {
        total_files += fingerprints.len();
        if let Some(convention) =
            conventions::discover_conventions(name, glob, fingerprints)
        {
            discovered_conventions.push(convention);
        }
    }

    // Phase 3: Check all conventions
    let check_results = checks::check_conventions(&discovered_conventions);

    // Phase 4: Build findings
    let all_findings = findings::build_findings(&check_results);

    // Phase 5: Build report
    let total_outliers: usize = discovered_conventions.iter().map(|c| c.outliers.len()).sum();
    let total_conforming: usize = discovered_conventions.iter().map(|c| c.conforming.len()).sum();
    let total_in_conventions = total_conforming + total_outliers;
    let alignment_score = if total_in_conventions > 0 {
        total_conforming as f32 / total_in_conventions as f32
    } else {
        1.0
    };

    let convention_reports: Vec<ConventionReport> = discovered_conventions
        .iter()
        .zip(check_results.iter())
        .map(|(conv, check)| ConventionReport {
            name: conv.name.clone(),
            glob: conv.glob.clone(),
            status: check.status.clone(),
            expected_methods: conv.expected_methods.clone(),
            expected_registrations: conv.expected_registrations.clone(),
            expected_interfaces: conv.expected_interfaces.clone(),
            conforming: conv.conforming.clone(),
            outliers: conv.outliers.clone(),
            total_files: conv.total_files,
            confidence: conv.confidence,
        })
        .collect();

    log_status!(
        "audit",
        "Complete: {} files, {} conventions, {} outliers (alignment: {:.0}%)",
        total_files,
        convention_reports.len(),
        total_outliers,
        alignment_score * 100.0
    );

    Ok(CodeAuditResult {
        component_id: component_id.to_string(),
        source_path: source_path.to_string(),
        summary: AuditSummary {
            files_scanned: total_files,
            conventions_detected: convention_reports.len(),
            outliers_found: total_outliers,
            alignment_score,
        },
        conventions: convention_reports,
        findings: all_findings,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn audit_nonexistent_path_returns_error() {
        let result = audit_path("/nonexistent/path/that/does/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn audit_empty_directory_returns_clean() {
        let dir = std::env::temp_dir().join("homeboy_audit_test_empty");
        let _ = fs::create_dir_all(&dir);

        let result = audit_path(dir.to_str().unwrap()).unwrap();
        assert_eq!(result.summary.files_scanned, 0);
        assert_eq!(result.summary.alignment_score, 1.0);
        assert!(result.conventions.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn audit_directory_with_convention() {
        let dir = std::env::temp_dir().join("homeboy_audit_test_conv");
        let steps = dir.join("steps");
        let _ = fs::create_dir_all(&steps);

        // Create 3 files: 2 follow pattern, 1 is an outlier
        fs::write(
            steps.join("step_a.php"),
            r#"<?php
class StepA {
    public function register() {}
    public function validate($input) {}
    public function execute($ctx) {}
}
"#,
        )
        .unwrap();

        fs::write(
            steps.join("step_b.php"),
            r#"<?php
class StepB {
    public function register() {}
    public function validate($input) {}
    public function execute($ctx) {}
}
"#,
        )
        .unwrap();

        fs::write(
            steps.join("step_c.php"),
            r#"<?php
class StepC {
    public function register() {}
    public function execute($ctx) {}
}
"#,
        )
        .unwrap();

        let result = audit_path(dir.to_str().unwrap()).unwrap();

        assert_eq!(result.summary.files_scanned, 3);
        assert!(result.summary.conventions_detected >= 1);
        assert!(result.summary.outliers_found >= 1);
        assert!(result.summary.alignment_score < 1.0);

        // Find the steps convention
        let steps_conv = result
            .conventions
            .iter()
            .find(|c| c.name == "Steps")
            .expect("Should find Steps convention");

        assert_eq!(steps_conv.total_files, 3);
        assert!(steps_conv.expected_methods.contains(&"register".to_string()));
        assert!(steps_conv.expected_methods.contains(&"execute".to_string()));
        assert_eq!(steps_conv.outliers.len(), 1);
        assert!(steps_conv.outliers[0].file.contains("step_c"));

        // Should have findings for the outlier
        assert!(!result.findings.is_empty());
        assert!(result
            .findings
            .iter()
            .any(|f| f.file.contains("step_c") && f.description.contains("validate")));

        let _ = fs::remove_dir_all(&dir);
    }
}
