use homeboy::extension::ExtensionManifest;

#[test]
fn test_source_url_accepts_manifest_source_url_alias() {
    let manifest: ExtensionManifest = serde_json::from_str(
        r#"{
  "name": "custom extension",
  "version": "1.0.0",
  "sourceUrl": "https://example.com/custom.git"
}"#,
    )
    .expect("manifest parses");

    assert_eq!(
        manifest.source_url.as_deref(),
        Some("https://example.com/custom.git")
    );
}

#[test]
fn test_source_url_keeps_snake_case_manifest_field() {
    let manifest: ExtensionManifest = serde_json::from_str(
        r#"{
  "name": "custom extension",
  "version": "1.0.0",
  "source_url": "https://example.com/custom.git"
}"#,
    )
    .expect("manifest parses");

    assert_eq!(
        manifest.source_url.as_deref(),
        Some("https://example.com/custom.git")
    );
}
