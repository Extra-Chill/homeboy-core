use serde_json::Value;
use std::process::Command;

#[test]
fn file_read_json_includes_size_metadata() {
    let home = tempfile::tempdir().expect("home tempdir");
    let project_root = tempfile::tempdir().expect("project tempdir");
    let project_id = "local-file-read";
    let content = "hello\nworld";

    std::fs::write(project_root.path().join("sample.txt"), content).expect("write sample file");

    let project_dir = home
        .path()
        .join(".config")
        .join("homeboy")
        .join("projects")
        .join(project_id);
    std::fs::create_dir_all(&project_dir).expect("create project config dir");
    let config = serde_json::json!({
        "base_path": project_root.path().to_string_lossy(),
    });
    std::fs::write(
        project_dir.join(format!("{project_id}.json")),
        serde_json::to_vec(&config).expect("serialize project config"),
    )
    .expect("write project config");

    let output = Command::new(env!("CARGO_BIN_EXE_homeboy"))
        .env("HOME", home.path())
        .args(["file", "read", project_id, "sample.txt", "--json"])
        .output()
        .expect("run homeboy file read");

    assert!(
        output.status.success(),
        "homeboy file read failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let payload: Value = serde_json::from_slice(&output.stdout).expect("parse json output");
    let expected_path = project_root
        .path()
        .join("sample.txt")
        .to_string_lossy()
        .to_string();

    assert_eq!(payload["success"], true);
    assert_eq!(payload["data"]["command"], "file.read");
    assert_eq!(payload["data"]["path"], expected_path);
    assert_eq!(payload["data"]["content"], content);
    assert_eq!(payload["data"]["size"], content.len() as i64);
}
