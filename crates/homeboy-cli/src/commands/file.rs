use clap::{Args, Subcommand};
use serde::Serialize;

use homeboy::remote_files::{self, FileEntry};

#[derive(Args)]
pub struct FileArgs {
    #[command(subcommand)]
    command: FileCommand,
}

#[derive(Subcommand)]
enum FileCommand {
    /// List directory contents
    List {
        /// Project ID
        project_id: String,
        /// Remote directory path
        path: String,
    },
    /// Read file content
    Read {
        /// Project ID
        project_id: String,
        /// Remote file path
        path: String,
    },
    /// Write content to file (from stdin)
    Write {
        /// Project ID
        project_id: String,
        /// Remote file path
        path: String,
    },
    /// Delete a file or directory
    Delete {
        /// Project ID
        project_id: String,
        /// Remote path to delete
        path: String,
        /// Delete directories recursively
        #[arg(short, long)]
        recursive: bool,
    },
    /// Rename or move a file
    Rename {
        /// Project ID
        project_id: String,
        /// Current path
        old_path: String,
        /// New path
        new_path: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOutput {
    command: String,
    project_id: String,
    base_path: Option<String>,
    path: Option<String>,
    old_path: Option<String>,
    new_path: Option<String>,
    recursive: Option<bool>,
    entries: Option<Vec<FileEntry>>,
    content: Option<String>,
    bytes_written: Option<usize>,
    stdout: Option<String>,
    stderr: Option<String>,
    exit_code: i32,
    success: bool,
}

pub fn run(
    args: FileArgs,
    _global: &crate::commands::GlobalArgs,
) -> homeboy::Result<(FileOutput, i32)> {
    match args.command {
        FileCommand::List { project_id, path } => list(&project_id, &path),
        FileCommand::Read { project_id, path } => read(&project_id, &path),
        FileCommand::Write { project_id, path } => write(&project_id, &path),
        FileCommand::Delete {
            project_id,
            path,
            recursive,
        } => delete(&project_id, &path, recursive),
        FileCommand::Rename {
            project_id,
            old_path,
            new_path,
        } => rename(&project_id, &old_path, &new_path),
    }
}

fn list(project_id: &str, path: &str) -> homeboy::Result<(FileOutput, i32)> {
    let result = remote_files::list(project_id, path)?;

    Ok((
        FileOutput {
            command: "file.list".to_string(),
            project_id: project_id.to_string(),
            base_path: result.base_path,
            path: Some(result.path),
            old_path: None,
            new_path: None,
            recursive: None,
            entries: Some(result.entries),
            content: None,
            bytes_written: None,
            stdout: None,
            stderr: None,
            exit_code: 0,
            success: true,
        },
        0,
    ))
}

fn read(project_id: &str, path: &str) -> homeboy::Result<(FileOutput, i32)> {
    let result = remote_files::read(project_id, path)?;

    Ok((
        FileOutput {
            command: "file.read".to_string(),
            project_id: project_id.to_string(),
            base_path: result.base_path,
            path: Some(result.path),
            old_path: None,
            new_path: None,
            recursive: None,
            entries: None,
            content: Some(result.content),
            bytes_written: None,
            stdout: None,
            stderr: None,
            exit_code: 0,
            success: true,
        },
        0,
    ))
}

fn write(project_id: &str, path: &str) -> homeboy::Result<(FileOutput, i32)> {
    let content = remote_files::read_stdin()?;
    let result = remote_files::write(project_id, path, &content)?;

    Ok((
        FileOutput {
            command: "file.write".to_string(),
            project_id: project_id.to_string(),
            base_path: result.base_path,
            path: Some(result.path),
            old_path: None,
            new_path: None,
            recursive: None,
            entries: None,
            content: None,
            bytes_written: Some(result.bytes_written),
            stdout: None,
            stderr: None,
            exit_code: 0,
            success: true,
        },
        0,
    ))
}

fn delete(project_id: &str, path: &str, recursive: bool) -> homeboy::Result<(FileOutput, i32)> {
    let result = remote_files::delete(project_id, path, recursive)?;

    Ok((
        FileOutput {
            command: "file.delete".to_string(),
            project_id: project_id.to_string(),
            base_path: result.base_path,
            path: Some(result.path),
            old_path: None,
            new_path: None,
            recursive: Some(result.recursive),
            entries: None,
            content: None,
            bytes_written: None,
            stdout: None,
            stderr: None,
            exit_code: 0,
            success: true,
        },
        0,
    ))
}

fn rename(project_id: &str, old_path: &str, new_path: &str) -> homeboy::Result<(FileOutput, i32)> {
    let result = remote_files::rename(project_id, old_path, new_path)?;

    Ok((
        FileOutput {
            command: "file.rename".to_string(),
            project_id: project_id.to_string(),
            base_path: result.base_path,
            path: None,
            old_path: Some(result.old_path),
            new_path: Some(result.new_path),
            recursive: None,
            entries: None,
            content: None,
            bytes_written: None,
            stdout: None,
            stderr: None,
            exit_code: 0,
            success: true,
        },
        0,
    ))
}
