use super::{CodeAuditResult, Finding, Severity};

#[derive(Debug, Clone, Copy)]
pub struct AuditConvergenceScoring {
    pub warning_weight: usize,
    pub info_weight: usize,
}

impl Default for AuditConvergenceScoring {
    fn default() -> Self {
        Self {
            warning_weight: 3,
            info_weight: 1,
        }
    }
}

impl AuditConvergenceScoring {
    fn severity_weight(&self, severity: &Severity) -> usize {
        match severity {
            Severity::Warning => self.warning_weight,
            Severity::Info => self.info_weight,
        }
    }

    fn weighted_finding_score(&self, result: &CodeAuditResult) -> usize {
        result
            .findings
            .iter()
            .map(|finding| self.severity_weight(&finding.severity))
            .sum()
    }
}

pub fn weighted_finding_score_with(
    result: &CodeAuditResult,
    scoring: AuditConvergenceScoring,
) -> usize {
    scoring.weighted_finding_score(result)
}

pub fn score_delta(
    before: &CodeAuditResult,
    after: &CodeAuditResult,
    scoring: AuditConvergenceScoring,
) -> isize {
    weighted_finding_score_with(before, scoring) as isize
        - weighted_finding_score_with(after, scoring) as isize
}

pub fn finding_fingerprint(finding: &Finding) -> String {
    format!(
        "{}::{:?}::{}::{}",
        finding.file, finding.kind, finding.convention, finding.description
    )
}
