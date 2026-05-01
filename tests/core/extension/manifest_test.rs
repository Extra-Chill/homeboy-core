use super::*;

fn manifest_with_sidecar_versions() -> ExtensionManifest {
    serde_json::from_value(serde_json::json!({
        "name": "Example",
        "version": "0.0.0",
        "lint": {
            "extension_script": "lint.sh",
            "findings_schema_version": "1"
        },
        "test": {
            "extension_script": "test.sh",
            "results_schema_version": "1",
            "failures_schema_version": "1"
        },
        "annotations_schema_version": "1"
    }))
    .expect("manifest should parse")
}

#[test]
fn test_lint_findings_schema_version() {
    let manifest = manifest_with_sidecar_versions();

    assert_eq!(manifest.lint_findings_schema_version(), Some("1"));
}

#[test]
fn test_test_results_schema_version() {
    let manifest = manifest_with_sidecar_versions();

    assert_eq!(manifest.test_results_schema_version(), Some("1"));
}

#[test]
fn test_test_failures_schema_version() {
    let manifest = manifest_with_sidecar_versions();

    assert_eq!(manifest.test_failures_schema_version(), Some("1"));
}

#[test]
fn test_structured_sidecars() {
    let manifest = manifest_with_sidecar_versions();

    let sidecars = manifest.structured_sidecars();
    assert_eq!(sidecars.len(), 4);
    assert_eq!(sidecars[0].name, "lint.findings");
    assert_eq!(sidecars[0].path, "lint-findings.json");
    assert_eq!(sidecars[1].name, "test.results");
    assert_eq!(sidecars[1].path, "test-results.json");
    assert_eq!(sidecars[2].name, "test.failures");
    assert_eq!(sidecars[2].path, "test-failures.json");
    assert_eq!(sidecars[3].name, "annotations");
    assert_eq!(sidecars[3].path, "annotations");
}

#[test]
fn test_missing_declarations_preserve_legacy_behavior() {
    let manifest: ExtensionManifest = serde_json::from_value(serde_json::json!({
        "name": "Example",
        "version": "0.0.0",
        "lint": { "extension_script": "lint.sh" },
        "test": { "extension_script": "test.sh" }
    }))
    .expect("manifest should parse");

    assert_eq!(manifest.lint_findings_schema_version(), None);
    assert_eq!(manifest.test_results_schema_version(), None);
    assert_eq!(manifest.test_failures_schema_version(), None);
    assert!(manifest.structured_sidecars().is_empty());
}
