use std::path::Path;

use super::{NewFindingRecord, NewRunRecord, ObservationStore, RunRecord, RunStatus};

pub struct ActiveObservation {
    store: ObservationStore,
    run: RunRecord,
    initial_metadata: serde_json::Value,
}

impl ActiveObservation {
    pub fn start(record: NewRunRecord) -> crate::Result<Self> {
        let store = ObservationStore::open_initialized()?;
        let initial_metadata = record.metadata_json.clone();
        let run = store.start_run(record)?;

        Ok(Self {
            store,
            run,
            initial_metadata,
        })
    }

    pub fn start_best_effort(record: NewRunRecord) -> Option<Self> {
        Self::start(record).ok()
    }

    pub fn store(&self) -> &ObservationStore {
        &self.store
    }

    pub fn run(&self) -> &RunRecord {
        &self.run
    }

    pub fn run_id(&self) -> &str {
        &self.run.id
    }

    pub fn component_id(&self) -> Option<&str> {
        self.run.component_id.as_deref()
    }

    pub fn rig_id(&self) -> Option<&str> {
        self.run.rig_id.as_deref()
    }

    pub fn initial_metadata(&self) -> &serde_json::Value {
        &self.initial_metadata
    }

    pub fn merged_metadata(&self, finish: serde_json::Value) -> serde_json::Value {
        merge_metadata(self.initial_metadata.clone(), finish)
    }

    pub fn finish(&self, status: RunStatus, metadata: Option<serde_json::Value>) {
        let _ = self.store.finish_run(self.run_id(), status, metadata);
    }

    pub fn finish_merged(&self, status: RunStatus, finish_metadata: serde_json::Value) {
        self.finish(status, Some(self.merged_metadata(finish_metadata)));
    }

    pub fn finish_error(&self, metadata: Option<serde_json::Value>) {
        self.finish(RunStatus::Error, metadata);
    }

    pub fn record_findings(&self, records: &[NewFindingRecord]) {
        let _ = self.store.record_findings(records);
    }

    pub fn record_artifact_if_file(&self, kind: &str, path: &Path) {
        if path.is_file() {
            let _ = self.store.record_artifact(self.run_id(), kind, path);
        }
    }

    pub fn store_path(&self) -> String {
        self.store
            .status()
            .map(|status| status.path)
            .unwrap_or_else(|_| "<unavailable>".to_string())
    }
}

pub fn merge_metadata(
    mut initial: serde_json::Value,
    finish: serde_json::Value,
) -> serde_json::Value {
    if let (Some(initial), Some(finish)) = (initial.as_object_mut(), finish.as_object()) {
        for (key, value) in finish {
            initial.insert(key.clone(), value.clone());
        }
    }
    initial
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_metadata_preserves_initial_and_overwrites_finish_keys() {
        let merged = merge_metadata(
            serde_json::json!({ "status": "running", "component": "homeboy" }),
            serde_json::json!({ "status": "pass", "exit_code": 0 }),
        );

        assert_eq!(merged["component"], "homeboy");
        assert_eq!(merged["status"], "pass");
        assert_eq!(merged["exit_code"], 0);
    }
}
