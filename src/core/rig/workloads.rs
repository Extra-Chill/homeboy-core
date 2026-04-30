//! Rig-owned extension workload resolution.

use std::path::{Path, PathBuf};

use super::spec::RigSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RigWorkloadKind {
    Bench,
    Trace,
}

pub fn extension_ids_for_workloads(rig_spec: &RigSpec, kind: RigWorkloadKind) -> Vec<String> {
    let mut ids: Vec<String> = match kind {
        RigWorkloadKind::Bench => rig_spec.bench_workloads.keys().cloned().collect(),
        RigWorkloadKind::Trace => rig_spec.trace_workloads.keys().cloned().collect(),
    };
    ids.sort();
    ids
}

pub fn workloads_for_extension(
    rig_spec: &RigSpec,
    kind: RigWorkloadKind,
    package_root: Option<&Path>,
    extension_id: &str,
) -> Vec<PathBuf> {
    let workloads = match kind {
        RigWorkloadKind::Bench => &rig_spec.bench_workloads,
        RigWorkloadKind::Trace => &rig_spec.trace_workloads,
    };

    workloads
        .get(extension_id)
        .into_iter()
        .flat_map(|paths| paths.iter())
        .map(|path| expand_workload_path(rig_spec, package_root, path))
        .collect()
}

fn expand_workload_path(rig_spec: &RigSpec, package_root: Option<&Path>, path: &str) -> PathBuf {
    let expanded = super::expand::expand_vars(rig_spec, path);
    let expanded = match package_root {
        Some(root) => expanded.replace("${package.root}", &root.to_string_lossy()),
        None => expanded,
    };
    PathBuf::from(expanded)
}

#[cfg(test)]
#[path = "../../../tests/core/rig/workloads_test.rs"]
mod workloads_test;
