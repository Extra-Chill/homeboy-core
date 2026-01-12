mod manifest;

pub use manifest::*;

use crate::config::AppPaths;
use crate::json::read_json_file_typed;
use std::fs;
use std::path::PathBuf;

pub fn load_plugin(id: &str) -> Option<PluginManifest> {
    let plugin_dir = AppPaths::plugin(id).ok()?;
    let manifest_path = plugin_dir.join("homeboy.json");

    if !manifest_path.exists() {
        return None;
    }

    let mut manifest: PluginManifest = read_json_file_typed(&manifest_path).ok()?;
    manifest.plugin_path = Some(plugin_dir.to_string_lossy().to_string());
    Some(manifest)
}

pub fn load_all_plugins() -> Vec<PluginManifest> {
    let Ok(plugins_dir) = AppPaths::plugins() else {
        return Vec::new();
    };

    if !plugins_dir.exists() {
        return Vec::new();
    }

    let Ok(entries) = fs::read_dir(&plugins_dir) else {
        return Vec::new();
    };

    let mut plugins = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let manifest_path = path.join("homeboy.json");
            if let Ok(mut manifest) = read_json_file_typed::<PluginManifest>(&manifest_path) {
                manifest.plugin_path = Some(path.to_string_lossy().to_string());
                plugins.push(manifest);
            }
        }
    }

    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    plugins
}

pub fn find_plugin_by_tool(tool: &str) -> Option<PluginManifest> {
    load_all_plugins()
        .into_iter()
        .find(|p| p.cli.as_ref().is_some_and(|c| c.tool == tool))
}

pub fn plugin_path(id: &str) -> PathBuf {
    AppPaths::plugin(id).unwrap_or_else(|_| PathBuf::from(id))
}

pub fn available_plugin_ids() -> Vec<String> {
    load_all_plugins().into_iter().map(|p| p.id).collect()
}
