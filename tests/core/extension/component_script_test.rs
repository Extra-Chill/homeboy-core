use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::commands::test::{run as run_test, TestArgs};
use crate::commands::utils::args::{
    BaselineArgs, ExtensionOverrideArgs, HiddenJsonArgs, PositionalComponentArgs, SettingArgs,
};
use crate::commands::GlobalArgs;
use crate::component::{Component, ComponentScriptsConfig};
use crate::engine::run_dir::RunDir;
use crate::extension::component_script::{
    run_component_scripts, run_component_scripts_with_env, run_component_scripts_with_run_dir,
    source_path,
};
use crate::extension::ExtensionCapability;
use crate::test_support::with_isolated_home;

fn component_script_args(root: &Path) -> PositionalComponentArgs {
    PositionalComponentArgs {
        component: Some("fixture".to_string()),
        path: Some(root.to_string_lossy().to_string()),
    }
}

fn test_command_args(root: &Path) -> TestArgs {
    TestArgs {
        comp: component_script_args(root),
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

fn write_component_script(root: &Path, name: &str, body: &str) {
    let script_dir = root.join("scripts");
    fs::create_dir_all(&script_dir).expect("script dir should be created");
    fs::write(script_dir.join(name), body).expect("script should be written");
}

fn script_component(root: &Path, command: &str) -> Component {
    let mut component = Component::new(
        "fixture".to_string(),
        root.to_string_lossy().to_string(),
        String::new(),
        None,
    );
    component.scripts = Some(ComponentScriptsConfig {
        test: vec![command.to_string()],
        ..Default::default()
    });
    component
}

#[test]
fn test_run_component_scripts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let component = script_component(dir.path(), "printf ok > marker");

    let output = run_component_scripts(&component, ExtensionCapability::Test, dir.path(), false)
        .expect("component script should run");

    assert!(output.success);
    assert_eq!(output.exit_code, 0);
    assert_eq!(fs::read_to_string(dir.path().join("marker")).unwrap(), "ok");
}

#[test]
fn test_run_component_scripts_with_env() {
    let dir = tempfile::tempdir().expect("temp dir");
    let component = script_component(dir.path(), "printf \"$EXTRA_VALUE\" > marker");

    let output = run_component_scripts_with_env(
        &component,
        ExtensionCapability::Test,
        dir.path(),
        false,
        &[("EXTRA_VALUE".to_string(), "ok".to_string())],
        &[],
    )
    .expect("component script should run with env");

    assert!(output.success);
    assert_eq!(fs::read_to_string(dir.path().join("marker")).unwrap(), "ok");
}

#[test]
fn test_run_component_scripts_with_run_dir() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_dir = RunDir::create().expect("run dir");
    let component = script_component(
        dir.path(),
        "test -n \"$HOMEBOY_RUN_DIR\" && printf ok > marker",
    );

    let output = run_component_scripts_with_run_dir(
        &component,
        ExtensionCapability::Test,
        dir.path(),
        &run_dir,
        false,
        &[],
        &[],
    )
    .expect("component script should run with run dir");

    assert!(output.success);
    assert_eq!(fs::read_to_string(dir.path().join("marker")).unwrap(), "ok");
    run_dir.cleanup();
}

#[test]
fn test_source_path() {
    let component = Component::new(
        "fixture".to_string(),
        "/component/path".to_string(),
        String::new(),
        None,
    );

    assert_eq!(source_path(&component, None), Path::new("/component/path"));
    assert_eq!(
        source_path(&component, Some("/override")),
        Path::new("/override")
    );
}

#[test]
fn command_dispatch_runs_component_script_before_extension_resolution() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(
        dir.path().join("homeboy.json"),
        r#"{
  "id": "fixture",
  "scripts": { "test": ["sh scripts/test.sh"] }
}"#,
    )
    .expect("homeboy.json should be written");
    write_component_script(
        dir.path(),
        "test.sh",
        "printf 'component script ran\n' > component-script-marker\n",
    );

    let (output, exit_code) =
        run_test(test_command_args(dir.path()), &GlobalArgs {}).expect("test script should run");

    assert_eq!(exit_code, 0);
    assert!(output.passed);
    assert!(dir.path().join("component-script-marker").exists());
}

#[test]
fn command_dispatch_falls_back_to_extension_when_component_script_is_absent() {
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
            .path()
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

        let (output, exit_code) = run_test(test_command_args(dir.path()), &GlobalArgs {})
            .expect("extension test should run");

        assert_eq!(exit_code, 0);
        assert!(output.passed);
        assert!(dir.path().join("extension-marker").exists());
    });
}
