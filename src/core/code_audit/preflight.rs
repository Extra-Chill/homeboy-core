#![allow(unused_imports)]

//! Compatibility shim for historical code_audit preflight imports.
//!
//! Preflight ownership now lives under `crate::refactor::auto::preflight`.

pub use crate::refactor::auto::preflight::{
    run_fix_preflight, run_insertion_preflight, run_new_file_preflight,
};
