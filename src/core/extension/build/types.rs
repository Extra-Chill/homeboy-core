//! types — extracted from mod.rs.

use crate::component::{self, Component};
use crate::core::extension::build::command;
use crate::core::extension::*;
use crate::engine::command::CapturedOutput;
use crate::error::{Error, Result};
use crate::extension::{self, exec_context, ExtensionCapability, ExtensionExecutionContext};
use crate::output::{BulkResult, BulkSummary, ItemOutcome};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct BuildOutput {
    pub command: String,
    pub component_id: String,
    pub build_command: String,
    #[serde(flatten)]
    pub output: CapturedOutput,
    pub success: bool,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum BuildResult {
    Single(BuildOutput),
    Bulk(BulkResult<BuildOutput>),
}
