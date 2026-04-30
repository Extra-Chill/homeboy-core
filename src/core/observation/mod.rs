//! Local observation store.
//!
//! Boundary: JSON/files describe desired state (`homeboy.json`, rig specs,
//! stack specs, baselines). SQLite stores observed state from command runs and
//! generated artifacts. This module only provides the storage substrate.

pub mod store;

pub use store::{
    ArtifactRecord, NewRunRecord, ObservationDbStatus, ObservationStore, RunListFilter, RunRecord,
    RunStatus, CURRENT_SCHEMA_VERSION,
};
