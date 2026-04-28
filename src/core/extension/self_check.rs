use crate::component::Component;
use crate::error::{Error, Result};
use crate::extension::ExtensionCapability;
use crate::server::{execute_local_command_passthrough, CommandOutput};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SelfCheckOutput {
    pub exit_code: i32,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub fn run_self_checks(
    component: &Component,
    capability: ExtensionCapability,
    source_path: &Path,
) -> Result<SelfCheckOutput> {
    let commands = component.self_check_commands(capability);
    if commands.is_empty() {
        return Err(Error::validation_invalid_argument(
            "self_checks",
            format!(
                "Component '{}' has no {} self-check commands configured",
                component.id,
                capability.label()
            ),
            None,
            None,
        ));
    }

    let working_dir = source_path.to_string_lossy();
    let mut stdout = String::new();
    let mut stderr = String::new();

    for command in commands {
        crate::log_status!(
            "self-check",
            "running {} self-check for {}: {}",
            capability.label(),
            component.id,
            command
        );
        let output = execute_self_check_command(command, &working_dir);
        stdout.push_str(&output.stdout);
        stderr.push_str(&output.stderr);

        if !output.success {
            return Ok(SelfCheckOutput {
                exit_code: output.exit_code,
                success: false,
                stdout,
                stderr,
            });
        }
    }

    Ok(SelfCheckOutput {
        exit_code: 0,
        success: true,
        stdout,
        stderr,
    })
}

fn execute_self_check_command(command: &str, working_dir: &str) -> CommandOutput {
    execute_local_command_passthrough(command, Some(working_dir), None)
}
