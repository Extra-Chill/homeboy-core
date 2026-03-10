pub use crate::code_audit::fixer::{
    apply_decompose_plans, apply_fix_policy, apply_fixes, apply_fixes_chunked, apply_new_files,
    apply_new_files_chunked, auto_apply_subset, generate_fixes, ApplyChunkResult, ApplyOptions,
    ChunkStatus, ChunkVerifier, Fix, FixPolicy, FixResult, FixSafetyTier, Insertion,
    InsertionKind, NewFile, PolicySummary, PreflightCheck, PreflightContext, PreflightReport,
    PreflightStatus, SkippedFile,
};
pub use crate::code_audit::preflight::{
    run_fix_preflight, run_insertion_preflight, run_new_file_preflight,
};
