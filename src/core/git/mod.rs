mod changes;
mod commits;
mod operations;
mod primitives;

pub use changes::*;
pub use commits::*;
pub use operations::*;
pub use primitives::*;

use std::process::Command;

use crate::error::Error;

fn execute_git(path: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("git").args(args).current_dir(path).output()
}

fn resolve_target(
    component_id: Option<&str>,
    path_override: Option<&str>,
) -> crate::error::Result<(String, String)> {
    let id = component_id.ok_or_else(|| {
        Error::validation_invalid_argument(
            "componentId",
            "Missing componentId",
            None,
            Some(vec![
                "Provide a component ID: homeboy git <command> <component-id>".to_string(),
                "List available components: homeboy component list".to_string(),
            ]),
        )
    })?;
    let path = if let Some(p) = path_override {
        p.to_string()
    } else {
        let comp = crate::component::load(id)?;
        comp.local_path
    };
    Ok((id.to_string(), path))
}
