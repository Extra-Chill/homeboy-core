//! Re-exports from server module for backward compatibility.
//!
//! SSH client, connection resolution, and local command execution
//! now live in `core::server`. This module re-exports them so existing
//! `use crate::ssh::*` imports continue to work.

pub use crate::server::*;
