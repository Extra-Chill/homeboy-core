use homeboy::commands::test_scope::{run, TestScopeArgs};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("homeboy-test-scope-{name}-{nanos}"))
}

#[test]
fn test_run() {
    let path = tmp_dir("run");
    assert!(!path.as_os_str().is_empty());

    let (output, exit_code) = run(
        TestScopeArgs {
            since: "HEAD~7".to_string(),
        },
        &homeboy::commands::GlobalArgs {},
    )
    .expect("test_scope run should succeed");

    assert_eq!(exit_code, 0);
    assert_eq!(output.status, "ready");
    assert_eq!(output.changed_since, "HEAD~7");
}
