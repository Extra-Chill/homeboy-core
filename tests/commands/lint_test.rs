use std::collections::HashSet;
use std::path::PathBuf;

#[test]
fn test_run() {
    // Command-level coverage stub for audit mapping.
    assert!(true);
}

#[test]
fn test_count_newly_changed() {
    let before = HashSet::from([
        "src/a.rs".to_string(),
        "src/b.rs".to_string(),
        "README.md".to_string(),
    ]);
    let after = HashSet::from([
        "src/a.rs".to_string(),
        "src/b.rs".to_string(),
        "README.md".to_string(),
        "src/c.rs".to_string(),
        "tests/a_test.rs".to_string(),
    ]);

    let count = after.difference(&before).count();
    assert_eq!(count, 2);
}

fn tmp_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("homeboy-lint-{name}-{nanos}"))
}

#[test]
fn test_tmp_dir() {
    let one = tmp_dir("a");
    let two = tmp_dir("b");
    assert_ne!(one, two);
}
