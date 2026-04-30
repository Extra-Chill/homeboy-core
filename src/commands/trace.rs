use clap::Args;
use std::path::PathBuf;

use homeboy::component::{Component, ScopedExtensionConfig};
use homeboy::engine::execution_context::{self, ResolveOptions};
use homeboy::engine::run_dir::RunDir;
use homeboy::extension::trace as extension_trace;
use homeboy::extension::trace::{TraceCommandOutput, TraceListWorkflowArgs, TraceRunWorkflowArgs};
use homeboy::extension::ExtensionCapability;
use homeboy::rig::{self, RigSpec};

use super::utils::args::{HiddenJsonArgs, PositionalComponentArgs, SettingArgs};
use super::{CmdResult, GlobalArgs};

#[derive(Args)]
pub struct TraceArgs {
    #[command(flatten)]
    comp: PositionalComponentArgs,

    /// Scenario ID to run, or `list` to discover available scenarios.
    pub scenario: String,

    /// Run trace against a rig-pinned component path after `rig check` passes.
    #[arg(long, value_name = "RIG_ID")]
    pub rig: Option<String>,

    #[command(flatten)]
    pub setting_args: SettingArgs,

    #[command(flatten)]
    pub _json: HiddenJsonArgs,

    /// Print compact machine-readable summary.
    #[arg(long)]
    pub json_summary: bool,

    /// Apply a patch file for this trace run, then reverse it afterward.
    #[arg(long = "overlay", value_name = "PATCH_FILE")]
    pub overlays: Vec<String>,

    /// Leave overlay changes in place after the trace run.
    #[arg(long)]
    pub keep_overlay: bool,
}

pub fn run(args: TraceArgs, _global: &GlobalArgs) -> CmdResult<TraceCommandOutput> {
    if args.scenario == "list" {
        return run_list(args);
    }

    let rig_context = load_rig_context(args.rig.as_deref())?;
    let effective_id = resolve_component_id(&args.comp, rig_context.as_ref().map(|c| &c.spec))?;
    let path_override = args.comp.path.clone().or_else(|| {
        rig_context
            .as_ref()
            .and_then(|context| rig_component_path(&context.spec, &effective_id))
    });
    let component_override = rig_context
        .as_ref()
        .and_then(|context| rig_component_for_trace(&context.spec, &effective_id));

    let ctx = execution_context::resolve_with_component(
        &ResolveOptions::with_capability_and_json(
            &effective_id,
            path_override.clone(),
            ExtensionCapability::Trace,
            args.setting_args.setting.clone(),
            args.setting_args.setting_json.clone(),
        ),
        component_override,
    )?;

    let rig_state = rig_context
        .as_ref()
        .map(|context| rig::snapshot_state(&context.spec));
    let run_dir = RunDir::create()?;
    let extra_workloads = rig_context
        .as_ref()
        .and_then(|context| {
            ctx.extension_id.as_deref().map(|id| {
                rig::workloads_for_extension(
                    &context.spec,
                    rig::RigWorkloadKind::Trace,
                    context.package_root.as_deref(),
                    id,
                )
            })
        })
        .unwrap_or_default();
    let workflow = extension_trace::run_trace_workflow(
        &ctx.component,
        TraceRunWorkflowArgs {
            component_label: effective_id,
            component_id: ctx.component_id.clone(),
            path_override,
            settings: settings_as_strings(&ctx.settings),
            settings_json: settings_as_json(&ctx.settings),
            scenario_id: args.scenario,
            json_summary: args.json_summary,
            rig_id: args.rig,
            overlays: args.overlays,
            keep_overlay: args.keep_overlay,
            extra_workloads,
        },
        &run_dir,
        rig_state.clone(),
    )?;

    Ok(extension_trace::from_main_workflow(
        workflow,
        rig_state,
        args.json_summary,
    ))
}

fn run_list(args: TraceArgs) -> CmdResult<TraceCommandOutput> {
    let rig_context = load_rig_context(args.rig.as_deref())?;
    let effective_id = resolve_component_id(&args.comp, rig_context.as_ref().map(|c| &c.spec))?;
    let path_override = args.comp.path.clone().or_else(|| {
        rig_context
            .as_ref()
            .and_then(|context| rig_component_path(&context.spec, &effective_id))
    });
    let component_override = rig_context
        .as_ref()
        .and_then(|context| rig_component_for_trace(&context.spec, &effective_id));

    let ctx = execution_context::resolve_with_component(
        &ResolveOptions::with_capability_and_json(
            &effective_id,
            path_override.clone(),
            ExtensionCapability::Trace,
            args.setting_args.setting.clone(),
            args.setting_args.setting_json.clone(),
        ),
        component_override,
    )?;

    let run_dir = RunDir::create()?;
    let extra_workloads = rig_context
        .as_ref()
        .and_then(|context| {
            ctx.extension_id.as_deref().map(|id| {
                rig::workloads_for_extension(
                    &context.spec,
                    rig::RigWorkloadKind::Trace,
                    context.package_root.as_deref(),
                    id,
                )
            })
        })
        .unwrap_or_default();
    let list = extension_trace::run_trace_list_workflow(
        &ctx.component,
        TraceListWorkflowArgs {
            component_label: effective_id.clone(),
            component_id: ctx.component_id.clone(),
            path_override,
            settings: settings_as_strings(&ctx.settings),
            settings_json: settings_as_json(&ctx.settings),
            rig_id: args.rig,
            extra_workloads,
        },
        &run_dir,
    )?;

    Ok(extension_trace::from_list_workflow(effective_id, list))
}

struct TraceRigContext {
    spec: RigSpec,
    package_root: Option<PathBuf>,
}

fn load_rig_context(rig_id: Option<&str>) -> homeboy::Result<Option<TraceRigContext>> {
    let Some(rig_id) = rig_id else {
        return Ok(None);
    };
    let spec = rig::load(rig_id)?;
    let check = rig::run_check(&spec)?;
    if !check.success {
        return Err(homeboy::Error::validation_invalid_argument(
            "--rig",
            format!(
                "rig '{}' check failed; fix the rig before running trace",
                rig_id
            ),
            None,
            None,
        ));
    }
    let package_root =
        rig::read_source_metadata(&spec.id).map(|metadata| PathBuf::from(metadata.package_path));
    Ok(Some(TraceRigContext { spec, package_root }))
}

fn resolve_component_id(
    comp: &PositionalComponentArgs,
    rig_spec: Option<&RigSpec>,
) -> homeboy::Result<String> {
    if let Some(id) = comp.id() {
        return Ok(id.to_string());
    }
    if let Some(spec) = rig_spec {
        if spec.components.len() == 1 {
            return Ok(spec.components.keys().next().unwrap().clone());
        }
        return Err(homeboy::Error::validation_invalid_argument(
            "component",
            format!(
                "rig '{}' has multiple components; pass the component id to trace",
                spec.id
            ),
            None,
            None,
        ));
    }
    comp.resolve_id()
}

fn rig_component_path(spec: &RigSpec, component_id: &str) -> Option<String> {
    let component = spec.components.get(component_id)?;
    Some(homeboy::rig::expand::expand_vars(spec, &component.path))
}

fn rig_component_for_trace(spec: &RigSpec, component_id: &str) -> Option<Component> {
    let component = spec.components.get(component_id)?;
    let mut extensions = component.extensions.clone().unwrap_or_default();
    for extension_id in rig::extension_ids_for_workloads(spec, rig::RigWorkloadKind::Trace) {
        extensions
            .entry(extension_id)
            .or_insert_with(ScopedExtensionConfig::default);
    }
    Some(Component {
        id: component_id.to_string(),
        local_path: rig_component_path(spec, component_id)
            .unwrap_or_else(|| component.path.clone()),
        remote_url: component.remote_url.clone(),
        triage_remote_url: component.triage_remote_url.clone(),
        extensions: if extensions.is_empty() {
            None
        } else {
            Some(extensions)
        },
        ..Default::default()
    })
}

fn settings_as_strings(settings: &[(String, serde_json::Value)]) -> Vec<(String, String)> {
    settings
        .iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(s) => Some((key.clone(), s.clone())),
            _ => None,
        })
        .collect()
}

fn settings_as_json(settings: &[(String, serde_json::Value)]) -> Vec<(String, serde_json::Value)> {
    settings
        .iter()
        .filter_map(|(key, value)| match value {
            serde_json::Value::String(_) => None,
            other => Some((key.clone(), other.clone())),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;

    use crate::test_support::with_isolated_home;

    use homeboy::component::ScopedExtensionConfig;
    use homeboy::rig::ComponentSpec;

    use super::*;

    #[test]
    fn rig_component_path_and_trace_env_are_threaded() {
        let mut components = HashMap::new();
        let mut extensions = HashMap::new();
        extensions.insert(
            "trace-extension".to_string(),
            ScopedExtensionConfig::default(),
        );
        components.insert(
            "studio".to_string(),
            ComponentSpec {
                path: "~/Developer/studio".to_string(),
                remote_url: Some("https://github.com/Automattic/studio".to_string()),
                triage_remote_url: None,
                stack: None,
                branch: None,
                extensions: Some(extensions),
            },
        );
        let spec = RigSpec {
            id: "studio-rig".to_string(),
            components,
            ..serde_json::from_str(r#"{"id":"studio-rig"}"#).unwrap()
        };

        let path = rig_component_path(&spec, "studio").expect("path resolves");
        assert!(path.contains("/Developer/studio"));
        let component = rig_component_for_trace(&spec, "studio").expect("component resolves");
        assert_eq!(component.id, "studio");
        assert_eq!(component.local_path, path);
        assert!(component.extensions.is_some());
    }

    #[test]
    fn rig_component_for_trace_synthesizes_trace_workload_extensions() {
        let rig_spec: RigSpec = serde_json::from_str(
            r#"{
                "id": "studio",
                "components": {
                    "studio": { "path": "/tmp/studio" }
                },
                "trace_workloads": {
                    "nodejs": ["/tmp/create-site.trace.mjs"]
                }
            }"#,
        )
        .expect("parse rig spec");

        let component = rig_component_for_trace(&rig_spec, "studio").expect("component");

        assert!(component
            .extensions
            .as_ref()
            .expect("extensions")
            .contains_key("nodejs"));
    }

    #[test]
    fn rig_trace_list_uses_rig_default_component_and_workloads() {
        with_isolated_home(|home| {
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (output, exit_code) = run_list(TraceArgs {
                comp: PositionalComponentArgs {
                    component: None,
                    path: None,
                },
                scenario: "list".to_string(),
                rig: Some("studio-rig".to_string()),
                setting_args: SettingArgs::default(),
                _json: HiddenJsonArgs::default(),
                json_summary: false,
                overlays: Vec::new(),
                keep_overlay: false,
            })
            .expect("rig trace list should run");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::List(result) => {
                    assert_eq!(result.component, "studio");
                    assert_eq!(result.component_id, "studio");
                    assert_eq!(result.count, 2);
                    assert_eq!(result.scenarios[0].id, "studio-app-create-site");
                    let expected_source = format!(
                        "{}/studio-app-create-site.trace.mjs",
                        component_dir.path().display()
                    );
                    assert_eq!(
                        result.scenarios[0].source.as_deref(),
                        Some(expected_source.as_str())
                    );
                }
                _ => panic!("expected list output"),
            }
        });
    }

    #[test]
    fn rig_trace_run_uses_rig_owned_workload_extension_without_component_link() {
        with_isolated_home(|home| {
            write_trace_extension(home);
            let component_dir = tempfile::TempDir::new().expect("component dir");
            write_trace_rig(home, "studio-rig", "studio", component_dir.path());

            let (output, exit_code) = run(
                TraceArgs {
                    comp: PositionalComponentArgs {
                        component: Some("studio".to_string()),
                        path: None,
                    },
                    scenario: "studio-app-create-site".to_string(),
                    rig: Some("studio-rig".to_string()),
                    setting_args: SettingArgs::default(),
                    _json: HiddenJsonArgs::default(),
                    json_summary: false,
                    overlays: Vec::new(),
                    keep_overlay: false,
                },
                &GlobalArgs {},
            )
            .expect("rig trace run should run");

            assert_eq!(exit_code, 0);
            match output {
                TraceCommandOutput::Run(result) => {
                    assert!(result.passed);
                    assert_eq!(result.component, "studio");
                    assert_eq!(
                        result.results.expect("results").scenario_id,
                        "studio-app-create-site"
                    );
                }
                _ => panic!("expected run output"),
            }
        });
    }

    fn write_trace_extension(home: &tempfile::TempDir) {
        let extension_dir = home
            .path()
            .join(".config")
            .join("homeboy")
            .join("extensions")
            .join("nodejs");
        fs::create_dir_all(&extension_dir).expect("mkdir extension");
        fs::write(
            extension_dir.join("nodejs.json"),
            r#"{
                "name": "Node.js",
                "version": "0.0.0",
                "trace": { "extension_script": "trace-runner.sh" }
            }"#,
        )
        .expect("write extension manifest");

        let script_path = extension_dir.join("trace-runner.sh");
        fs::write(
            &script_path,
            r#"#!/bin/sh
set -eu
scenario_ids=""
old_ifs="$IFS"
IFS=":"
for workload in ${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}; do
  name="$(basename "$workload")"
  name="${name%%.trace.*}"
  name="${name%.*}"
  if [ -n "$scenario_ids" ]; then
    scenario_ids="$scenario_ids $name"
  else
    scenario_ids="$name"
  fi
done
IFS="$old_ifs"

if [ "$HOMEBOY_TRACE_LIST_ONLY" = "1" ]; then
  cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenarios":[
JSON
  comma=""
  old_ifs="$IFS"
  IFS=":"
  for workload in ${HOMEBOY_TRACE_EXTRA_WORKLOADS:-}; do
    name="$(basename "$workload")"
    name="${name%%.trace.*}"
    name="${name%.*}"
    printf '%s{"id":"%s","source":"%s"}' "$comma" "$name" "$workload" >> "$HOMEBOY_TRACE_RESULTS_FILE"
    comma=","
  done
  IFS="$old_ifs"
  printf ']}' >> "$HOMEBOY_TRACE_RESULTS_FILE"
  exit 0
fi

case " $scenario_ids " in
  *" $HOMEBOY_TRACE_SCENARIO "*) ;;
  *) printf 'unknown scenario %s\n' "$HOMEBOY_TRACE_SCENARIO" >&2; exit 3 ;;
esac

cat > "$HOMEBOY_TRACE_RESULTS_FILE" <<JSON
{"component_id":"$HOMEBOY_COMPONENT_ID","scenario_id":"$HOMEBOY_TRACE_SCENARIO","status":"pass","timeline":[],"assertions":[],"artifacts":[]}
JSON
"#,
        )
        .expect("write trace script");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script_path)
                .expect("script metadata")
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script_path, permissions).expect("chmod script");
        }
    }

    fn write_trace_rig(
        home: &tempfile::TempDir,
        rig_id: &str,
        component_id: &str,
        path: &std::path::Path,
    ) {
        let rig_dir = home.path().join(".config").join("homeboy").join("rigs");
        fs::create_dir_all(&rig_dir).expect("mkdir rigs");
        fs::write(
            rig_dir.join(format!("{}.json", rig_id)),
            format!(
                r#"{{
                    "components": {{
                        "{component_id}": {{ "path": "{}" }}
                    }},
                    "trace_workloads": {{ "nodejs": [
                        "${{components.{component_id}.path}}/studio-app-create-site.trace.mjs",
                        "${{components.{component_id}.path}}/studio-list-sites.trace.mjs"
                    ] }}
                }}"#,
                path.display()
            ),
        )
        .expect("write rig");
    }
}
