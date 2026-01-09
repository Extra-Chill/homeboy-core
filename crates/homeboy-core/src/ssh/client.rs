use std::process::{Command, Stdio};
use crate::config::{AppPaths, ServerConfig};
use crate::Result;

pub struct SshClient {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub key_path: String,
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub exit_code: i32,
}

impl SshClient {
    pub fn from_server(server: &ServerConfig, server_id: &str) -> Result<Self> {
        let key_path = AppPaths::key(server_id);

        if !key_path.exists() {
            return Err(crate::Error::Ssh(format!(
                "SSH key not found for server '{}'. Configure SSH in Homeboy.app first.",
                server_id
            )));
        }

        Ok(Self {
            host: server.host.clone(),
            user: server.user.clone(),
            port: server.port,
            key_path: key_path.to_string_lossy().to_string(),
        })
    }

    pub fn execute(&self, command: &str) -> CommandOutput {
        let output = Command::new("/usr/bin/ssh")
            .args([
                "-i", &self.key_path,
                "-o", "StrictHostKeyChecking=no",
                "-o", "BatchMode=yes",
                "-p", &self.port.to_string(),
                &format!("{}@{}", self.user, self.host),
                command,
            ])
            .output();

        match output {
            Ok(out) => CommandOutput {
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                success: out.status.success(),
                exit_code: out.status.code().unwrap_or(-1),
            },
            Err(e) => CommandOutput {
                stdout: String::new(),
                stderr: format!("SSH error: {}", e),
                success: false,
                exit_code: -1,
            },
        }
    }

    pub fn execute_interactive(&self, command: Option<&str>) -> i32 {
        let mut args = vec![
            "-i".to_string(),
            self.key_path.clone(),
            "-o".to_string(),
            "StrictHostKeyChecking=no".to_string(),
            "-p".to_string(),
            self.port.to_string(),
            format!("{}@{}", self.user, self.host),
        ];

        if let Some(cmd) = command {
            args.push(cmd.to_string());
        }

        let status = Command::new("/usr/bin/ssh")
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        match status {
            Ok(s) => s.code().unwrap_or(-1),
            Err(_) => -1,
        }
    }
}

pub fn execute_local_command(command: &str) -> CommandOutput {
    let output = Command::new("/bin/bash")
        .args(["-c", command])
        .output();

    match output {
        Ok(out) => CommandOutput {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            success: out.status.success(),
            exit_code: out.status.code().unwrap_or(-1),
        },
        Err(e) => CommandOutput {
            stdout: String::new(),
            stderr: format!("Command error: {}", e),
            success: false,
            exit_code: -1,
        },
    }
}
