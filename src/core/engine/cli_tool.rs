use serde::Serialize;
use std::collections::HashMap;

use crate::component::{self, Component};
use crate::context::resolve_project_ssh;
use crate::engine::executor;
use crate::engine::shell;
use crate::engine::template::{render_map, TemplateVars};
use crate::engine::text;
use crate::error::ErrorCode;
use crate::extension::{find_extension_by_tool, CliAutoFlag, CliConfig};
use crate::project::{self, Project};
use crate::server;
use crate::server::{execute_local_command, CommandOutput};
use crate::{Error, Result};

#[derive(Serialize, Clone)]

pub struct CliToolResult {
    pub tool: String,
    pub extension_id: String,
    pub identifier: String,
    pub target_domain: Option<String>,
    pub executed_command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub fn run(tool: &str, identifier: &str, args: &[String]) -> Result<CliToolResult> {
    // Normalize args: split quoted strings containing spaces.
    // This ensures both syntaxes work identically:
    //   homeboy wp extra-chill:events datamachine pipelines list
    //   homeboy wp extra-chill:events "datamachine pipelines list"
    let args = shell::normalize_args(args);

    // Parse project:subtarget syntax
    let (project_id, embedded_subtarget) = crate::engine::text::split_identifier(identifier);

    // Build args with embedded subtarget prepended if present
    let full_args: Vec<String> = match embedded_subtarget {
        Some(sub) => std::iter::once(sub.to_string())
            .chain(args.iter().cloned())
            .collect(),
        None => args.to_vec(),
    };

    // Try component first (uses original identifier for component lookup)
    if let Some(result) = try_run_for_component(tool, identifier, &args) {
        return result;
    }

    run_for_project(tool, project_id, &full_args)
}

fn try_run_for_component(
    tool: &str,
    identifier: &str,
    args: &[String],
) -> Option<Result<CliToolResult>> {
    match component::resolve_effective(Some(identifier), None, None) {
        Ok(component) => {
            let extension = find_extension_by_tool(tool)?;
            let cli_config = extension.cli.as_ref()?;

            let command = build_component_command(&component, cli_config, &extension, args);
            let output = execute_local_command(&command);

            Some(Ok(CliToolResult {
                tool: tool.to_string(),
                extension_id: extension.id.clone(),
                identifier: identifier.to_string(),
                target_domain: None,
                executed_command: command,
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.exit_code,
            }))
        }
        Err(e) if e.code == ErrorCode::ComponentNotFound => None,
        Err(e) => Some(Err(e)),
    }
}

fn build_component_command(
    component: &Component,
    cli_config: &CliConfig,
    extension: &crate::extension::ExtensionManifest,
    args: &[String],
) -> String {
    let mut variables = HashMap::new();
    variables.insert(
        TemplateVars::SITE_PATH.to_string(),
        component.local_path.clone(),
    );
    variables.insert(
        TemplateVars::CLI_PATH.to_string(),
        cli_config
            .default_cli_path
            .clone()
            .unwrap_or_else(|| cli_config.tool.clone()),
    );
    variables.insert(TemplateVars::ARGS.to_string(), shell::quote_args(args));

    if let Some(ref path) = extension.extension_path {
        variables.insert(TemplateVars::EXTENSION_PATH.to_string(), path.clone());
    }

    render_map(&cli_config.command_template, &variables)
}

fn run_for_project(tool: &str, project_id: &str, args: &[String]) -> Result<CliToolResult> {
    run_for_project_with_executor(tool, project_id, args, project::load, execute_local_command)
}

fn run_for_project_with_executor(
    tool: &str,
    project_id: &str,
    args: &[String],
    project_loader: fn(&str) -> Result<Project>,
    local_executor: fn(&str) -> CommandOutput,
) -> Result<CliToolResult> {
    if args.is_empty() {
        return Err(Error::validation_missing_argument(vec![
            "command".to_string()
        ]));
    }

    let extension = find_extension_by_tool(tool).ok_or_else(|| {
        Error::validation_invalid_argument(
            "tool",
            format!("No extension provides tool '{}'", tool),
            Some(tool.to_string()),
            None,
        )
    })?;

    let cli_config = extension.cli.as_ref().ok_or_else(|| {
        Error::config(format!(
            "Extension '{}' does not have CLI configuration",
            extension.id
        ))
    })?;

    let project = project_loader(project_id)?;

    let (target_domain, command_args) = resolve_subtarget(&project, args)?;

    if command_args.is_empty() {
        return Err(Error::validation_missing_argument(vec![
            "command".to_string()
        ]));
    }

    // Try direct execution first (bypasses shell escaping issues)
    let (output, executed_command) = if project.server_id.as_ref().is_none_or(|s| s.is_empty()) {
        let result = executor::execute_for_project_direct(
            &project,
            cli_config,
            &extension.id,
            &command_args,
            &target_domain,
        );
        match result {
            Ok(cmd_output) => (
                cmd_output,
                format!("{} {}", cli_config.tool, command_args.join(" ")),
            ),
            Err(_) => {
                // Fallback to shell execution if direct fails
                let (_, rendered_cmd) =
                    build_project_command(&project, cli_config, &extension.id, args)?;
                (local_executor(&rendered_cmd), rendered_cmd)
            }
        }
    } else {
        let ctx = resolve_project_ssh(project_id)?;
        let (_, rendered_cmd) = build_project_command(&project, cli_config, &extension.id, args)?;
        let cmd_output = ctx.client.execute(&rendered_cmd);
        (cmd_output, rendered_cmd)
    };

    Ok(CliToolResult {
        tool: tool.to_string(),
        extension_id: extension.id,
        identifier: project_id.to_string(),
        target_domain: Some(target_domain),
        executed_command,
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    })
}

fn build_project_command(
    project: &Project,
    cli_config: &CliConfig,
    extension_id: &str,
    args: &[String],
) -> Result<(String, String)> {
    let base_path = project
        .base_path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| Error::config("Base path not configured".to_string()))?;

    let (target_domain, command_args) = resolve_subtarget(project, args)?;

    if command_args.is_empty() {
        return Err(Error::validation_missing_argument(vec![
            "command".to_string()
        ]));
    }

    let cli_path = project::project_cli_path(project).unwrap_or_else(|| {
        cli_config
            .default_cli_path
            .clone()
            .unwrap_or_else(|| cli_config.tool.clone())
    });

    let (command_template, template_global_args) =
        extract_wp_cli_global_template_args(cli_config, &cli_config.command_template);

    let mut variables = HashMap::new();
    variables.insert(TemplateVars::PROJECT_ID.to_string(), project.id.clone());
    variables.insert(TemplateVars::DOMAIN.to_string(), target_domain.clone());
    variables.insert(TemplateVars::SITE_PATH.to_string(), base_path);
    variables.insert(TemplateVars::CLI_PATH.to_string(), cli_path);

    let mut rendered_args = project_cli_flags(
        project,
        cli_config,
        extension_id,
        project_server_user(project).as_deref(),
        true,
    );
    rendered_args.extend(
        template_global_args
            .into_iter()
            .map(|arg| render_map(&arg, &variables)),
    );
    rendered_args.extend(command_args);
    variables.insert(
        TemplateVars::ARGS.to_string(),
        shell::quote_args(&rendered_args),
    );

    // Add extension_path so {{extension_path}} resolves in command templates
    let extension_dir = crate::extension::extension_path(extension_id);
    if extension_dir.exists() {
        variables.insert(
            TemplateVars::EXTENSION_PATH.to_string(),
            extension_dir.to_string_lossy().to_string(),
        );
    }

    let rendered = render_map(&command_template, &variables);

    Ok((target_domain, rendered))
}

fn extract_wp_cli_global_template_args(
    cli_config: &CliConfig,
    command_template: &str,
) -> (String, Vec<String>) {
    if cli_config.tool != "wp" {
        return (command_template.to_string(), Vec::new());
    }

    let mut globals = Vec::new();
    let mut parts = Vec::new();
    let mut after_args = false;

    for part in command_template.split_whitespace() {
        if part == "{{args}}" {
            after_args = true;
            parts.push(part.to_string());
            continue;
        }

        if after_args && is_wp_cli_global_arg(part) {
            globals.push(part.to_string());
            continue;
        }

        parts.push(part.to_string());
    }

    (parts.join(" "), globals)
}

fn is_wp_cli_global_arg(arg: &str) -> bool {
    matches!(arg, "--path" | "--url" | "--user" | "--allow-root")
        || arg.starts_with("--path=")
        || arg.starts_with("--url=")
        || arg.starts_with("--user=")
}

fn project_cli_flags(
    project: &Project,
    cli_config: &CliConfig,
    extension_id: &str,
    server_user: Option<&str>,
    quote_setting_values: bool,
) -> Vec<String> {
    let mut flags = Vec::new();

    if let Some(extension_config) = project
        .extensions
        .as_ref()
        .and_then(|m| m.get(extension_id))
    {
        for (setting_key, flag_template) in &cli_config.settings_flags {
            if let Some(flag) = extension_config
                .settings
                .get(setting_key)
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|value_str| {
                    let value = if quote_setting_values {
                        shell::quote_arg(value_str)
                    } else {
                        value_str.to_string()
                    };
                    flag_template.replace("{{value}}", &value)
                })
            {
                flags.push(flag);
            }
        }
    }

    flags.extend(
        matching_auto_flags(cli_config, server_user)
            .into_iter()
            .map(str::to_string),
    );

    flags
}

fn project_server_user(project: &Project) -> Option<String> {
    let server_id = project.server_id.as_ref().filter(|s| !s.is_empty())?;
    server::load(server_id).ok().map(|svr| svr.user)
}

fn matching_auto_flags<'a>(cli_config: &'a CliConfig, server_user: Option<&str>) -> Vec<&'a str> {
    cli_config
        .auto_flags
        .iter()
        .filter(|auto_flag| auto_flag_matches(auto_flag, server_user))
        .map(|auto_flag| auto_flag.flag.as_str())
        .collect()
}

fn auto_flag_matches(auto_flag: &CliAutoFlag, server_user: Option<&str>) -> bool {
    if let Some(expected_user) = auto_flag.when.server_user.as_deref() {
        return server_user == Some(expected_user);
    }

    true
}

fn resolve_subtarget(project: &Project, args: &[String]) -> Result<(String, Vec<String>)> {
    let require_domain = || {
        Error::validation_invalid_argument(
            "domain",
            "This operation requires a domain to be configured on the project",
            Some(project.id.clone()),
            None,
        )
    };

    if project.sub_targets.is_empty() {
        let domain = project.domain.clone().ok_or_else(require_domain)?;
        return Ok((domain, args.to_vec()));
    }

    let Some(sub_id) = args.first() else {
        let subtarget_list = project
            .sub_targets
            .iter()
            .map(|t| {
                let slug = project::slugify_id(&t.name).unwrap_or_else(|_| t.name.clone());
                format!("- {} (use: {})", t.name, slug)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(Error::validation_invalid_argument(
            "subtarget",
            format!(
                "This project has subtargets configured. You must specify which subtarget to use.\n\n\
                 Available subtargets for project '{}':\n{}\n\n\
                 Syntax: homeboy <tool> {}:<subtarget> <command>...\n\
                     OR  homeboy <tool> {} <subtarget> <command>...\n\n\
                 Commands can be quoted or unquoted:\n  \
                   homeboy wp {}:events post list\n  \
                   homeboy wp {}:events \"post list\"",
                project.id, subtarget_list, project.id, project.id, project.id, project.id
            ),
            Some(project.id.clone()),
            None,
        ));
    };

    if let Some(subtarget) = project.sub_targets.iter().find(|t| {
        project::slugify_id(&t.name).ok().as_deref() == Some(sub_id)
            || text::identifier_eq(&t.name, sub_id)
    }) {
        return Ok((subtarget.domain.clone(), args[1..].to_vec()));
    }

    let subtarget_list = project
        .sub_targets
        .iter()
        .map(|t| {
            let slug = project::slugify_id(&t.name).unwrap_or_else(|_| t.name.clone());
            format!("- {} (use: {})", t.name, slug)
        })
        .collect::<Vec<_>>()
        .join("\n");
    Err(Error::validation_invalid_argument(
        "subtarget",
        format!(
            "Subtarget '{}' not found. Available subtargets for project '{}':\n{}\n\n\
             Syntax: homeboy <tool> {}:<subtarget> <command>...\n\
                 OR  homeboy <tool> {} <subtarget> <command>...\n\n\
             Commands can be quoted or unquoted:\n  \
               homeboy wp {}:events post list\n  \
               homeboy wp {}:events \"post list\"",
            sub_id, project.id, subtarget_list, project.id, project.id, project.id, project.id
        ),
        Some(project.id.clone()),
        None,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component::ScopedExtensionConfig;
    use crate::extension::{CliAutoFlagCondition, CliHelpConfig};

    fn cli_config(auto_flags: Vec<CliAutoFlag>) -> CliConfig {
        CliConfig {
            tool: "wp".to_string(),
            display_name: "WP-CLI".to_string(),
            command_template: "{{cliPath}} {{args}}".to_string(),
            default_cli_path: Some("wp".to_string()),
            working_dir_template: None,
            settings_flags: HashMap::new(),
            auto_flags,
            help: None::<CliHelpConfig>,
        }
    }

    #[test]
    fn auto_flags_match_server_user_conditions() {
        let config = cli_config(vec![
            CliAutoFlag {
                when: CliAutoFlagCondition {
                    server_user: Some("root".to_string()),
                },
                flag: "--allow-root".to_string(),
            },
            CliAutoFlag {
                when: CliAutoFlagCondition {
                    server_user: Some("deploy".to_string()),
                },
                flag: "--as-deploy".to_string(),
            },
        ]);

        assert_eq!(
            matching_auto_flags(&config, Some("root")),
            vec!["--allow-root"]
        );
        assert_eq!(
            matching_auto_flags(&config, Some("deploy")),
            vec!["--as-deploy"]
        );
        assert!(matching_auto_flags(&config, Some("www-data")).is_empty());
        assert!(matching_auto_flags(&config, None).is_empty());
    }

    #[test]
    fn auto_flags_with_empty_conditions_always_apply() {
        let config = cli_config(vec![CliAutoFlag {
            when: CliAutoFlagCondition::default(),
            flag: "--global-flag".to_string(),
        }]);

        assert_eq!(
            matching_auto_flags(&config, Some("root")),
            vec!["--global-flag"]
        );
        assert_eq!(matching_auto_flags(&config, None), vec!["--global-flag"]);
    }

    #[test]
    fn empty_manifest_auto_flags_do_not_apply_implicit_extension_flags() {
        let config = cli_config(Vec::new());

        assert!(matching_auto_flags(&config, Some("root")).is_empty());
        assert!(matching_auto_flags(&config, Some("deploy")).is_empty());
    }

    #[test]
    fn cli_config_deserializes_manifest_auto_flags() {
        let config: CliConfig = serde_json::from_str(
            r#"{
                "tool": "wp",
                "display_name": "WP-CLI",
                "command_template": "{{cliPath}} {{args}}",
                "auto_flags": [
                    { "when": { "server_user": "root" }, "flag": "--allow-root" }
                ]
            }"#,
        )
        .expect("parse cli config");

        assert_eq!(config.auto_flags.len(), 1);
        assert_eq!(
            config.auto_flags[0].when.server_user.as_deref(),
            Some("root")
        );
        assert_eq!(config.auto_flags[0].flag, "--allow-root");
    }

    #[test]
    fn project_cli_path_overrides_manifest_default() {
        let mut project = Project {
            id: "sandbox".to_string(),
            domain: Some("example.com".to_string()),
            base_path: Some("/home/wpdev/public_html".to_string()),
            cli_path: Some("/home/wpdev/public_html/bin/wp".to_string()),
            ..Default::default()
        };

        let config = cli_config(Vec::new());
        let (_, command) = build_project_command(
            &project,
            &config,
            "wordpress",
            &["core".into(), "version".into()],
        )
        .expect("build command");

        assert!(command.starts_with("/home/wpdev/public_html/bin/wp core version"));

        project.cli_path = None;
        let (_, command) = build_project_command(
            &project,
            &config,
            "wordpress",
            &["core".into(), "version".into()],
        )
        .expect("build command");

        assert!(command.starts_with("wp core version"));
    }

    #[test]
    fn project_cli_flags_render_before_wp_subcommand_args() {
        let mut settings_flags = HashMap::new();
        settings_flags.insert("path".to_string(), "--path={{value}}".to_string());
        settings_flags.insert("url".to_string(), "--url={{value}}".to_string());

        let mut config = cli_config(vec![CliAutoFlag {
            when: CliAutoFlagCondition::default(),
            flag: "--allow-root".to_string(),
        }]);
        config.settings_flags = settings_flags;

        let mut wordpress_settings = HashMap::new();
        wordpress_settings.insert(
            "path".to_string(),
            serde_json::Value::String("/htdocs/__wp__".to_string()),
        );
        wordpress_settings.insert(
            "url".to_string(),
            serde_json::Value::String("balanced-jovial-earth.wpcloudstation.dev".to_string()),
        );

        let mut extensions = HashMap::new();
        extensions.insert(
            "wordpress".to_string(),
            ScopedExtensionConfig {
                settings: wordpress_settings,
                ..Default::default()
            },
        );

        let project = Project {
            id: "intelligence-horse".to_string(),
            domain: Some("balanced-jovial-earth.wpcloudstation.dev".to_string()),
            base_path: Some("/htdocs/__wp__".to_string()),
            extensions: Some(extensions),
            ..Default::default()
        };

        let (_, command) = build_project_command(
            &project,
            &config,
            "wordpress",
            &["core".into(), "version".into()],
        )
        .expect("build command");

        let core_pos = command.find(" core version").expect("core version args");
        for flag in [
            "--path=/htdocs/__wp__",
            "--url=balanced-jovial-earth.wpcloudstation.dev",
            "--allow-root",
        ] {
            let flag_pos = command.find(flag).expect("global flag");
            assert!(
                flag_pos < core_pos,
                "expected {flag} before WP-CLI subcommand in {command}"
            );
        }
    }

    #[test]
    fn wp_cli_template_globals_render_before_subcommand_args() {
        let mut config = cli_config(Vec::new());
        config.command_template =
            "{{cliPath}} {{args}} --path={{sitePath}} --url={{domain}}".to_string();

        let project = Project {
            id: "intelligence-horse".to_string(),
            domain: Some("balanced-jovial-earth.wpcloudstation.dev".to_string()),
            base_path: Some("/htdocs/__wp__".to_string()),
            ..Default::default()
        };

        let (_, command) = build_project_command(
            &project,
            &config,
            "wordpress",
            &["core".into(), "version".into()],
        )
        .expect("build command");

        assert_eq!(
            command,
            "wp --path=/htdocs/__wp__ --url=balanced-jovial-earth.wpcloudstation.dev core version"
        );
    }
}
