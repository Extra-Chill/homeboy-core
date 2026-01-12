use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy_core::plugin::{load_all_plugins, load_plugin, PluginManifest};

use crate::commands::CmdResult;

#[derive(Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    command: PluginCommand,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// List available plugins
    List,
    /// Show details of a specific plugin
    Show {
        /// Plugin ID
        plugin_id: String,
    },
}

pub fn run(args: PluginArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<PluginOutput> {
    match args.command {
        PluginCommand::List => list(),
        PluginCommand::Show { plugin_id } => show(&plugin_id),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOutput {
    pub command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<Vec<PluginEntry>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<PluginDetail>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub has_cli: bool,
    pub commands: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginDetail {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub icon: String,
    pub has_cli: bool,
    pub commands: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_tool: Option<String>,
    pub default_pinned_files: Vec<String>,
    pub default_pinned_logs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_path: Option<String>,
}

fn list() -> CmdResult<PluginOutput> {
    let plugins = load_all_plugins();

    let entries: Vec<PluginEntry> = plugins
        .iter()
        .map(|plugin| PluginEntry {
            id: plugin.id.clone(),
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            description: plugin.description.clone(),
            has_cli: plugin.has_cli(),
            commands: plugin.commands.clone(),
        })
        .collect();

    Ok((
        PluginOutput {
            command: "plugin.list".to_string(),
            plugin_id: None,
            plugins: Some(entries),
            plugin: None,
        },
        0,
    ))
}

fn show(plugin_id: &str) -> CmdResult<PluginOutput> {
    let plugin = load_plugin(plugin_id).ok_or_else(|| {
        homeboy_core::Error::other(format!("Plugin '{}' not found", plugin_id))
    })?;

    let detail = plugin_to_detail(&plugin);

    Ok((
        PluginOutput {
            command: "plugin.show".to_string(),
            plugin_id: Some(plugin_id.to_string()),
            plugins: None,
            plugin: Some(detail),
        },
        0,
    ))
}

fn plugin_to_detail(plugin: &PluginManifest) -> PluginDetail {
    PluginDetail {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        version: plugin.version.clone(),
        description: plugin.description.clone(),
        author: plugin.author.clone(),
        icon: plugin.icon.clone(),
        has_cli: plugin.has_cli(),
        commands: plugin.commands.clone(),
        cli_tool: plugin.cli.as_ref().map(|cli| cli.tool.clone()),
        default_pinned_files: plugin.default_pinned_files.clone(),
        default_pinned_logs: plugin.default_pinned_logs.clone(),
        plugin_path: plugin.plugin_path.clone(),
    }
}
