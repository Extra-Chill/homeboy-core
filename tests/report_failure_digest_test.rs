use std::fs;
use std::path::{Path, PathBuf};

use homeboy::commands::report::{render_failure_digest_from_args, FailureDigestArgs};

const LINT_JSON: &str = include_str!("fixtures/failure_digest/lint.json");
const TEST_JSON: &str = include_str!("fixtures/failure_digest/test.json");
const AUDIT_JSON: &str = include_str!("fixtures/failure_digest/audit.json");
const TOOLING_JSON: &str = include_str!("fixtures/failure_digest/tooling.json");

fn tmp_dir(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("homeboy-failure-digest-{name}-{nanos}"))
}

fn write_file(dir: &Path, name: &str, body: &str) {
    fs::write(dir.join(name), body).expect("fixture should be written");
}

fn render(dir: &Path, results: &str, autofix_enabled: bool, autofix_attempted: bool) -> String {
    render_failure_digest_from_args(&FailureDigestArgs {
        output_dir: dir.to_string_lossy().to_string(),
        results: results.to_string(),
        run_url: Some("https://github.com/Extra-Chill/homeboy/actions/runs/123".to_string()),
        tooling_json: None,
        commands: Some("audit,lint,test".to_string()),
        autofix_commands: None,
        autofix_enabled,
        autofix_attempted,
        format: "markdown".to_string(),
    })
    .expect("failure digest should render")
}

#[test]
fn renders_lint_failure_digest_from_fixture() {
    let dir = tmp_dir("lint");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    write_file(&dir, "lint.json", LINT_JSON);

    let markdown = render(&dir, r#"{"lint":"fail"}"#, false, false);

    assert!(markdown.contains("## Failure Digest"));
    assert!(markdown.contains("### Lint Failure Digest"));
    assert!(markdown.contains("- Lint summary: **3 lint finding(s)**"));
    assert!(markdown.contains("<details><summary>Top lint violations</summary>"));
    assert!(markdown
        .contains("- Full lint log: https://github.com/Extra-Chill/homeboy/actions/runs/123"));
    assert!(!markdown.contains("### Test Failure Digest"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_test_failure_digest_from_fixture() {
    let dir = tmp_dir("test");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    write_file(&dir, "test.json", TEST_JSON);

    let markdown = render(&dir, r#"{"test":"fail"}"#, false, false);

    assert!(markdown.contains("### Test Failure Digest"));
    assert!(markdown.contains("- Failed tests: **2**"));
    assert!(markdown
        .contains("1. test_widget_renders — expected widget output — tests/widget_test.rs:42"));
    assert!(markdown.contains(
        "2. test_widget_handles_empty_state — empty state missing — tests/widget_test.rs"
    ));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_audit_failure_digest_from_fixture() {
    let dir = tmp_dir("audit");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    write_file(&dir, "audit.json", AUDIT_JSON);

    let markdown = render(&dir, r#"{"audit":"fail"}"#, false, false);

    assert!(markdown.contains("### Audit Failure Digest"));
    assert!(markdown.contains("- Alignment score: **0.812**"));
    assert!(markdown.contains("- Severity counts: **high: 1, low: 1, medium: 1**"));
    assert!(markdown.contains("- New findings since baseline: **1**"));
    assert!(markdown.contains("1. **src/report.rs** — new report module lacks tests (`abc123`)"));
    assert!(markdown.contains("**src/render.rs** — god_file — file is too large"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_mixed_failures_with_autofix_enabled_not_attempted() {
    let dir = tmp_dir("mixed-autofix");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    write_file(&dir, "lint.json", LINT_JSON);
    write_file(&dir, "test.json", TEST_JSON);

    let markdown = render(
        &dir,
        r#"{"lint":"fail","test":"fail","audit":"pass"}"#,
        true,
        false,
    );

    assert!(markdown.contains("### Lint Failure Digest"));
    assert!(markdown.contains("### Test Failure Digest"));
    assert!(markdown.contains("- Overall: **auto_fixable**"));
    assert!(markdown.contains("- Autofix enabled: **yes**"));
    assert!(markdown.contains("- Auto-fixable failed commands:"));
    assert!(markdown.contains("  - `lint`"));
    assert!(markdown.contains("  - `test`"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_mixed_failures_after_autofix_attempted_as_human_needed() {
    let dir = tmp_dir("attempted");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    write_file(&dir, "lint.json", LINT_JSON);

    let markdown = render(&dir, r#"{"lint":"fail"}"#, true, true);

    assert!(markdown.contains("- Overall: **human_needed**"));
    assert!(markdown.contains("- Autofix attempted this run: **yes**"));
    assert!(markdown.contains("- Human-needed failed commands:"));
    assert!(markdown.contains("  - `lint`"));
    assert!(markdown.contains("- Failed commands with available automated fixes:"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_json_renders_explicit_structured_details_unavailable() {
    let dir = tmp_dir("missing");
    fs::create_dir_all(&dir).expect("temp dir should exist");

    let markdown = render(
        &dir,
        r#"{"lint":"fail","test":"fail","audit":"fail"}"#,
        false,
        false,
    );

    assert!(markdown.contains("- No structured lint details available."));
    assert!(markdown.contains("- No structured test failure details available."));
    assert!(markdown.contains("- No structured audit findings available."));
    assert!(markdown
        .contains("- Full audit log: https://github.com/Extra-Chill/homeboy/actions/runs/123"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn renders_tooling_metadata_from_json_file() {
    let dir = tmp_dir("tooling");
    fs::create_dir_all(&dir).expect("temp dir should exist");
    let tooling_path = dir.join("tooling.json");
    write_file(&dir, "tooling.json", TOOLING_JSON);

    let markdown = render_failure_digest_from_args(&FailureDigestArgs {
        output_dir: dir.to_string_lossy().to_string(),
        results: r#"{"lint":"pass"}"#.to_string(),
        run_url: None,
        tooling_json: Some(tooling_path.to_string_lossy().to_string()),
        commands: Some("lint".to_string()),
        autofix_commands: None,
        autofix_enabled: false,
        autofix_attempted: false,
        format: "markdown".to_string(),
    })
    .expect("failure digest should render");

    assert!(markdown.contains("### Tooling metadata"));
    assert!(markdown.contains("- action_repository: `Extra-Chill/homeboy-action`"));
    assert!(markdown.contains("- extension_id: `wordpress`"));

    let _ = fs::remove_dir_all(&dir);
}
