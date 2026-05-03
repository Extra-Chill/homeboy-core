use homeboy::commands::lint::{run as run_lint, LintArgs};
use homeboy::commands::test::{run as run_test, TestArgs};
use homeboy::commands::utils::args::{
    BaselineArgs, ExtensionOverrideArgs, HiddenJsonArgs, PositionalComponentArgs, SettingArgs,
};
use homeboy::commands::GlobalArgs;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Mutex;

static HOME_LOCK: Mutex<()> = Mutex::new(());

fn write_component(root: &Path, self_checks: &str) {
    fs::write(
        root.join("homeboy.json"),
        format!(
            r#"{{
  "id": "fixture",
  "self_checks": {}
}}"#,
            self_checks
        ),
    )
    .expect("homeboy.json should be written");
}

fn write_component_with_scripts(root: &Path, scripts: &str) {
    fs::write(
        root.join("homeboy.json"),
        format!(
            r#"{{
  "id": "fixture",
  "scripts": {}
}}"#,
            scripts
        ),
    )
    .expect("homeboy.json should be written");
}

fn with_isolated_home<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let old_home = std::env::var_os("HOME");
    let home = tempfile::tempdir().expect("home tempdir");

    std::env::set_var("HOME", home.path());
    let result = f(home.path());

    if let Some(value) = old_home {
        std::env::set_var("HOME", value);
    } else {
        std::env::remove_var("HOME");
    }

    result
}

fn write_script(root: &Path, name: &str, body: &str) {
    let script_dir = root.join("scripts");
    fs::create_dir_all(&script_dir).expect("script dir should be created");
    fs::write(script_dir.join(name), body).expect("script should be written");
}

fn component_args(root: &Path) -> PositionalComponentArgs {
    PositionalComponentArgs {
        component: Some("fixture".to_string()),
        path: Some(root.to_string_lossy().to_string()),
    }
}

fn lint_args(root: &Path) -> LintArgs {
    LintArgs {
        comp: component_args(root),
        extension_override: ExtensionOverrideArgs::default(),
        summary: false,
        file: None,
        glob: None,
        changed_only: false,
        changed_since: None,
        errors_only: false,
        sniffs: None,
        exclude_sniffs: None,
        category: None,
        fix: false,
        setting_args: SettingArgs::default(),
        baseline_args: BaselineArgs::default(),
        _json: HiddenJsonArgs::default(),
        json_summary: false,
    }
}

fn test_args(root: &Path) -> TestArgs {
    TestArgs {
        comp: component_args(root),
        extension_override: ExtensionOverrideArgs::default(),
        skip_lint: false,
        coverage: false,
        coverage_min: None,
        baseline_args: BaselineArgs::default(),
        analyze: false,
        drift: false,
        write: false,
        since: "HEAD~10".to_string(),
        changed_since: None,
        setting_args: SettingArgs::default(),
        args: Vec::new(),
        _json: HiddenJsonArgs::default(),
        json_summary: false,
    }
}

#[test]
fn lint_runs_declared_self_check_without_extensions() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_component(dir.path(), r#"{ "lint": ["sh scripts/lint.sh"] }"#);
    write_script(dir.path(), "lint.sh", "printf 'lint self-check ran\\n'\n");

    let (output, exit_code) =
        run_lint(lint_args(dir.path()), &GlobalArgs {}).expect("lint self-check should run");

    assert_eq!(exit_code, 0);
    assert!(output.passed);
    assert_eq!(output.component, "fixture");
}

#[test]
fn test_runs_declared_self_check_without_extensions() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_component(dir.path(), r#"{ "test": ["sh scripts/test.sh"] }"#);
    write_script(dir.path(), "test.sh", "printf 'test self-check ran\\n'\n");

    let (output, exit_code) =
        run_test(test_args(dir.path()), &GlobalArgs {}).expect("test self-check should run");

    assert_eq!(exit_code, 0);
    assert!(output.passed);
    assert_eq!(output.component, "fixture");
}

#[test]
fn test_runs_component_script_before_extension_resolution() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_component_with_scripts(dir.path(), r#"{ "test": ["sh scripts/test.sh"] }"#);
    write_script(
        dir.path(),
        "test.sh",
        "printf 'component script ran\n' > component-script-marker\n",
    );

    let (output, exit_code) =
        run_test(test_args(dir.path()), &GlobalArgs {}).expect("test script should run");

    assert_eq!(exit_code, 0);
    assert!(output.passed);
    assert!(dir.path().join("component-script-marker").exists());
}

#[test]
fn test_falls_back_to_extension_when_component_script_is_absent() {
    with_isolated_home(|home| {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(
            dir.path().join("homeboy.json"),
            r#"{
  "id": "fixture",
  "extensions": { "fixture-extension": {} }
}"#,
        )
        .expect("homeboy.json should be written");

        let extension_dir = home
            .join(".config")
            .join("homeboy")
            .join("extensions")
            .join("fixture-extension");
        fs::create_dir_all(&extension_dir).expect("extension dir should be created");
        fs::write(
            extension_dir.join("fixture-extension.json"),
            r#"{
  "name": "Fixture extension",
  "version": "1.0.0",
  "test": { "extension_script": "test.sh" }
}"#,
        )
        .expect("extension manifest should be written");
        let extension_script = extension_dir.join("test.sh");
        fs::write(
            &extension_script,
            "#!/bin/sh\nprintf 'extension ran\n' > \"$HOMEBOY_COMPONENT_PATH/extension-marker\"\n",
        )
        .expect("extension script should be written");
        let mut perms = fs::metadata(&extension_script)
            .expect("extension script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&extension_script, perms)
            .expect("extension script should be executable");

        let (output, exit_code) =
            run_test(test_args(dir.path()), &GlobalArgs {}).expect("extension test should run");

        assert_eq!(exit_code, 0);
        assert!(output.passed);
        assert!(dir.path().join("extension-marker").exists());
    });
}

#[test]
fn non_zero_self_check_fails_command_and_surfaces_output() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_component(dir.path(), r#"{ "test": ["sh scripts/fail.sh"] }"#);
    write_script(
        dir.path(),
        "fail.sh",
        "printf 'visible failure stdout\\n'\nprintf 'visible failure stderr\\n' >&2\nexit 7\n",
    );

    let (output, exit_code) = run_test(test_args(dir.path()), &GlobalArgs {})
        .expect("test self-check failure should return structured output");

    assert_eq!(exit_code, 7);
    assert!(!output.passed);
    assert_eq!(output.status, "failed");
    let raw = output
        .raw_output
        .expect("failure should include raw output");
    assert!(raw.stdout_tail.contains("visible failure stdout"));
    assert!(raw.stderr_tail.contains("visible failure stderr"));
}

#[test]
fn missing_extension_and_self_check_keeps_existing_error() {
    let dir = tempfile::tempdir().expect("temp dir");
    write_component(dir.path(), r#"{}"#);

    let err = match run_lint(lint_args(dir.path()), &GlobalArgs {}) {
        Ok(_) => panic!("lint without extension or self-check should fail"),
        Err(err) => err,
    };

    assert_eq!(err.code.as_str(), "extension.unsupported");
    assert!(
        err.to_string()
            .contains("No extension provider configured for component 'fixture'"),
        "unexpected error: {err}"
    );
}
