//! Public output types for Homeboy command responses.
//!
//! This module contains all types that are part of the public API
//! for command output. These are used by CLI commands and consumers
//! of the homeboy library.

use serde::{Deserialize, Serialize};

// ============================================================================
// Create Operations
// ============================================================================

/// Result of a single create operation.
#[derive(Debug, Clone)]
pub struct CreateResult<T> {
    pub id: String,
    pub entity: T,
}

/// Unified output for create operations (single or bulk).
#[derive(Debug, Clone)]
pub enum CreateOutput<T> {
    Single(CreateResult<T>),
    Bulk(BatchResult),
}

// ============================================================================
// Merge Operations
// ============================================================================

/// Unified output for merge operations (single or bulk).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MergeOutput {
    Single(MergeResult),
    Bulk(BatchResult),
}

/// Result of a config merge operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub id: String,
    pub updated_fields: Vec<String>,
}

/// Result of a config remove operation.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    pub id: String,
    pub removed_from: Vec<String>,
}

// ============================================================================
// Batch Operations
// ============================================================================

/// Summary of a batch create/update operation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    pub created: u32,
    pub updated: u32,
    pub skipped: u32,
    pub errors: u32,
    pub items: Vec<BatchResultItem>,
}

/// Individual item result within a batch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchResultItem {
    pub id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl BatchResult {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_created(&mut self, id: String) {
        self.created += 1;
        self.items.push(BatchResultItem {
            id,
            status: "created".to_string(),
            error: None,
        });
    }

    pub fn record_updated(&mut self, id: String) {
        self.updated += 1;
        self.items.push(BatchResultItem {
            id,
            status: "updated".to_string(),
            error: None,
        });
    }

    pub fn record_skipped(&mut self, id: String) {
        self.skipped += 1;
        self.items.push(BatchResultItem {
            id,
            status: "skipped".to_string(),
            error: None,
        });
    }

    pub fn record_error(&mut self, id: String, error: String) {
        self.errors += 1;
        self.items.push(BatchResultItem {
            id,
            status: "error".to_string(),
            error: Some(error),
        });
    }
}

// ============================================================================
// Bulk Operations (for commands that process multiple items)
// ============================================================================

/// Standardized bulk execution result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult<T: Serialize> {
    pub action: String,
    pub results: Vec<ItemOutcome<T>>,
    pub summary: BulkSummary,
}

/// Outcome for a single item in a bulk operation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemOutcome<T: Serialize> {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(flatten)]
    pub result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Summary of bulk operation results.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkSummary {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
}
