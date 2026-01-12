use crate::build::detect_zip_single_root_dir;
use crate::module::DeployVerification;
use crate::shell;
use crate::ssh::SshClient;
use crate::template::{render_map, TemplateVars};
use crate::Result;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Result of a deployment operation
pub struct DeployResult {
    pub success: bool,
    pub exit_code: i32,
    pub error: Option<String>,
}

impl DeployResult {
    fn success(exit_code: i32) -> Self {
        Self {
            success: true,
            exit_code,
            error: None,
        }
    }

    fn failure(exit_code: i32, error: String) -> Self {
        Self {
            success: false,
            exit_code,
            error: Some(error),
        }
    }
}

/// Main entry point - inspects artifact path and deploys appropriately
pub fn deploy_artifact(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
    base_path: &str,
    verification: Option<&DeployVerification>,
) -> Result<DeployResult> {
    if local_path.is_dir() {
        deploy_directory(ssh_client, local_path, remote_path)
    } else if local_path.extension().is_some_and(|e| e == "zip") {
        deploy_zip(ssh_client, local_path, remote_path, base_path, verification)
    } else if is_tarball(local_path, &[".tar.gz", ".tgz"]) {
        deploy_tarball(ssh_client, local_path, remote_path, "xzf")
    } else if is_tarball(local_path, &[".tar.bz2", ".tbz2"]) {
        deploy_tarball(ssh_client, local_path, remote_path, "xjf")
    } else if is_tarball(local_path, &[".tar"]) {
        deploy_tarball(ssh_client, local_path, remote_path, "xf")
    } else {
        deploy_file(ssh_client, local_path, remote_path)
    }
}

fn is_tarball(path: &Path, extensions: &[&str]) -> bool {
    path.to_str()
        .is_some_and(|p| extensions.iter().any(|ext| p.ends_with(ext)))
}

/// Deploy a directory recursively via scp -r
pub fn deploy_directory(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
) -> Result<DeployResult> {
    let parent = Path::new(remote_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(remote_path);

    let mkdir_cmd = format!("mkdir -p {}", shell::quote_path(parent));
    let mkdir_output = ssh_client.execute(&mkdir_cmd);
    if !mkdir_output.success {
        return Ok(DeployResult::failure(
            mkdir_output.exit_code,
            format!("Failed to create remote directory: {}", mkdir_output.stderr),
        ));
    }

    scp_recursive(ssh_client, local_path, remote_path)
}

/// Deploy a ZIP archive with optional verification
pub fn deploy_zip(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
    base_path: &str,
    verification: Option<&DeployVerification>,
) -> Result<DeployResult> {
    let zip_filename = local_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!(".homeboy-{}", name))
        .unwrap_or_else(|| ".homeboy-archive.zip".to_string());

    // Detect if ZIP has a single root directory
    let zip_root_dir = detect_zip_single_root_dir(local_path).ok().flatten();

    let install_basename = Path::new(remote_path)
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or_default();

    // Smart extraction: if ZIP root matches install basename, unzip to parent
    let (unzip_target_dir, final_install_dir) = if zip_root_dir
        .as_deref()
        .is_some_and(|root| root == install_basename)
    {
        let parent = Path::new(remote_path)
            .parent()
            .and_then(|v| v.to_str())
            .unwrap_or(remote_path)
            .to_string();
        (parent, remote_path.to_string())
    } else {
        (remote_path.to_string(), remote_path.to_string())
    };

    let upload_dir = if unzip_target_dir != final_install_dir {
        &unzip_target_dir
    } else {
        remote_path
    };

    let upload_path = format!("{}/{}", upload_dir, zip_filename);

    // Create target directory
    let mkdir_cmd = format!("mkdir -p {}", shell::quote_path(&unzip_target_dir));
    let mkdir_output = ssh_client.execute(&mkdir_cmd);
    if !mkdir_output.success {
        return Ok(DeployResult::failure(
            mkdir_output.exit_code,
            format!("Failed to create remote directory: {}", mkdir_output.stderr),
        ));
    }

    // Upload ZIP to temp location
    let upload_result = scp_file(ssh_client, local_path, &upload_path)?;
    if !upload_result.success {
        return Ok(upload_result);
    }

    // Calculate cleanup target for verification
    let cleanup_target_dir = if let Some(ref root) = zip_root_dir {
        if unzip_target_dir != final_install_dir {
            format!("{}/{}", unzip_target_dir, root)
        } else {
            final_install_dir.clone()
        }
    } else {
        final_install_dir.clone()
    };

    // With verification: cleanup old files before extraction
    if verification.is_some()
        && cleanup_target_dir.starts_with(base_path)
        && cleanup_target_dir != base_path
    {
        let cleanup_cmd = format!(
            "rm -rf {} && mkdir -p {}",
            shell::quote_path(&cleanup_target_dir),
            shell::quote_path(&cleanup_target_dir)
        );
        let cleanup_output = ssh_client.execute(&cleanup_cmd);
        if !cleanup_output.success {
            return Ok(DeployResult::failure(
                cleanup_output.exit_code,
                format!("Failed to cleanup before extraction: {}", cleanup_output.stderr),
            ));
        }
    }

    // Extract and remove temp ZIP
    let extract_cmd = format!(
        "cd {} && unzip -o {} && rm {}",
        shell::quote_path(&unzip_target_dir),
        shell::quote_path(&zip_filename),
        shell::quote_path(&zip_filename)
    );
    let extract_output = ssh_client.execute(&extract_cmd);
    if !extract_output.success {
        return Ok(DeployResult::failure(
            extract_output.exit_code,
            format!("Failed to extract ZIP: {}", extract_output.stderr),
        ));
    }

    // Run verification if configured
    if let Some(v) = verification {
        if let Some(ref verify_cmd_template) = v.verify_command {
            let mut vars = HashMap::new();
            vars.insert(TemplateVars::TARGET_DIR.to_string(), cleanup_target_dir.clone());
            let verify_cmd = render_map(verify_cmd_template, &vars);

            let verify_output = ssh_client.execute(&verify_cmd);
            if !verify_output.success || verify_output.stdout.trim().is_empty() {
                let error_msg = v
                    .verify_error_message
                    .as_ref()
                    .map(|msg| render_map(msg, &vars))
                    .unwrap_or_else(|| {
                        format!("Deploy verification failed for {}", cleanup_target_dir)
                    });
                return Ok(DeployResult::failure(1, error_msg));
            }
        }
    }

    Ok(DeployResult::success(0))
}

/// Deploy a tarball (upload, extract, cleanup temp file)
pub fn deploy_tarball(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
    tar_flags: &str,
) -> Result<DeployResult> {
    let tarball_filename = local_path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| format!(".homeboy-{}", name))
        .unwrap_or_else(|| ".homeboy-archive.tar.gz".to_string());

    let mkdir_cmd = format!("mkdir -p {}", shell::quote_path(remote_path));
    let mkdir_output = ssh_client.execute(&mkdir_cmd);
    if !mkdir_output.success {
        return Ok(DeployResult::failure(
            mkdir_output.exit_code,
            format!("Failed to create remote directory: {}", mkdir_output.stderr),
        ));
    }

    let upload_path = format!("{}/{}", remote_path, tarball_filename);
    let upload_result = scp_file(ssh_client, local_path, &upload_path)?;
    if !upload_result.success {
        return Ok(upload_result);
    }

    let extract_cmd = format!(
        "cd {} && tar {} {} && rm {}",
        shell::quote_path(remote_path),
        tar_flags,
        shell::quote_path(&tarball_filename),
        shell::quote_path(&tarball_filename)
    );

    let extract_output = ssh_client.execute(&extract_cmd);
    if !extract_output.success {
        return Ok(DeployResult::failure(
            extract_output.exit_code,
            format!("Failed to extract tarball: {}", extract_output.stderr),
        ));
    }

    Ok(DeployResult::success(0))
}

/// Deploy a single file via scp
pub fn deploy_file(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
) -> Result<DeployResult> {
    let parent = Path::new(remote_path)
        .parent()
        .and_then(|p| p.to_str())
        .unwrap_or(remote_path);

    let mkdir_cmd = format!("mkdir -p {}", shell::quote_path(parent));
    let mkdir_output = ssh_client.execute(&mkdir_cmd);
    if !mkdir_output.success {
        return Ok(DeployResult::failure(
            mkdir_output.exit_code,
            format!("Failed to create remote directory: {}", mkdir_output.stderr),
        ));
    }

    scp_file(ssh_client, local_path, remote_path)
}

fn scp_file(ssh_client: &SshClient, local_path: &Path, remote_path: &str) -> Result<DeployResult> {
    let mut scp_args: Vec<String> = vec![];

    if let Some(identity_file) = &ssh_client.identity_file {
        scp_args.push("-i".to_string());
        scp_args.push(identity_file.clone());
    }

    if ssh_client.port != 22 {
        scp_args.push("-P".to_string());
        scp_args.push(ssh_client.port.to_string());
    }

    scp_args.push(local_path.to_string_lossy().to_string());
    scp_args.push(format!(
        "{}@{}:{}",
        ssh_client.user,
        ssh_client.host,
        shell::quote_path(remote_path)
    ));

    let output = Command::new("scp").args(&scp_args).output();

    match output {
        Ok(output) if output.status.success() => Ok(DeployResult::success(0)),
        Ok(output) => Ok(DeployResult::failure(
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )),
        Err(err) => Ok(DeployResult::failure(1, err.to_string())),
    }
}

fn scp_recursive(
    ssh_client: &SshClient,
    local_path: &Path,
    remote_path: &str,
) -> Result<DeployResult> {
    let mut scp_args: Vec<String> = vec!["-r".to_string()];

    if let Some(identity_file) = &ssh_client.identity_file {
        scp_args.push("-i".to_string());
        scp_args.push(identity_file.clone());
    }

    if ssh_client.port != 22 {
        scp_args.push("-P".to_string());
        scp_args.push(ssh_client.port.to_string());
    }

    scp_args.push(local_path.to_string_lossy().to_string());
    scp_args.push(format!(
        "{}@{}:{}",
        ssh_client.user,
        ssh_client.host,
        shell::quote_path(remote_path)
    ));

    let output = Command::new("scp").args(&scp_args).output();

    match output {
        Ok(output) if output.status.success() => Ok(DeployResult::success(0)),
        Ok(output) => Ok(DeployResult::failure(
            output.status.code().unwrap_or(1),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )),
        Err(err) => Ok(DeployResult::failure(1, err.to_string())),
    }
}
