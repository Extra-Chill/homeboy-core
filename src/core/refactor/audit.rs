//! Compatibility shim for historical `crate::core::refactor::audit` imports.
//!
//! Audit refactor ownership now lives under `crate::refactor::plan::audit`.

pub use crate::refactor::plan::audit::{
    build_chunk_verifier, finding_fingerprint, rewrite_callers_after_dedup,
    run_audit_refactor, score_delta, weighted_finding_score_with, AuditConvergenceScoring,
    AuditRefactorIterationSummary, AuditRefactorOutcome, AuditVerificationToggles,
};
