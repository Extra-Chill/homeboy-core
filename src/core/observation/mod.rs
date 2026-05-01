//! Local observation store.
//!
//! Boundary: JSON/files describe desired state (`homeboy.json`, rig specs,
//! stack specs, baselines). SQLite stores observed state from command runs and
//! generated artifacts. This module only provides the storage substrate.

pub mod records;
pub mod store;

pub use records::{
    ArtifactRecord, FindingListFilter, FindingRecord, NewFindingRecord, NewRunRecord,
    NewTraceRunRecord, NewTraceSpanRecord, RunListFilter, RunRecord, RunStatus, TraceRunRecord,
    TraceSpanRecord,
};
pub use store::{ObservationDbStatus, ObservationStore, CURRENT_SCHEMA_VERSION};
