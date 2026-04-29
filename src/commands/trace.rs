use clap::Args;

use homeboy::component::Component;
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
    let list = extension_trace::run_trace_list_workflow(
        &ctx.component,
        TraceListWorkflowArgs {
            component_label: effective_id.clone(),
            component_id: ctx.component_id.clone(),
            path_override,
            settings: settings_as_strings(&ctx.settings),
            settings_json: settings_as_json(&ctx.settings),
            rig_id: args.rig,
        },
        &run_dir,
    )?;

    Ok(extension_trace::from_list_workflow(effective_id, list))
}

struct TraceRigContext {
    spec: RigSpec,
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
    Ok(Some(TraceRigContext { spec }))
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
    Some(Component {
        id: component_id.to_string(),
        local_path: rig_component_path(spec, component_id)
            .unwrap_or_else(|| component.path.clone()),
        remote_url: component.remote_url.clone(),
        triage_remote_url: component.triage_remote_url.clone(),
        extensions: component.extensions.clone(),
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
}
