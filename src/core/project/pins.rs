use serde::Serialize;

use crate::error::{Error, Result};

use super::{load, pin, save, unpin, PinOptions, PinType, Project};

pub struct PinUpdateOptions {
    pub label: Option<String>,
    pub tail_lines: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPinListItem {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_lines: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPinChange {
    pub path: String,
    pub r#type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectPinOutput {
    pub action: String,
    pub project_id: String,
    pub r#type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ProjectPinListItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub added: Option<ProjectPinChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<ProjectPinChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated: Option<ProjectPinListItem>,
}

pub fn list_pins(project_id: &str, pin_type: PinType) -> Result<ProjectPinOutput> {
    let project = load(project_id)?;

    let (items, type_string) = match pin_type {
        PinType::File => (
            project
                .remote_files
                .pinned_files
                .iter()
                .map(|file| ProjectPinListItem {
                    path: file.path.clone(),
                    label: file.label.clone(),
                    display_name: file.display_name().to_string(),
                    tail_lines: None,
                })
                .collect(),
            "file",
        ),
        PinType::Log => (
            project
                .remote_logs
                .pinned_logs
                .iter()
                .map(|log| ProjectPinListItem {
                    path: log.path.clone(),
                    label: log.label.clone(),
                    display_name: log.display_name().to_string(),
                    tail_lines: Some(log.tail_lines),
                })
                .collect(),
            "log",
        ),
    };

    Ok(ProjectPinOutput {
        action: "list".to_string(),
        project_id: project_id.to_string(),
        r#type: type_string.to_string(),
        items: Some(items),
        added: None,
        removed: None,
        updated: None,
    })
}

pub fn add_pin(
    project_id: &str,
    pin_type: PinType,
    path: &str,
    options: PinOptions,
) -> Result<ProjectPinOutput> {
    let type_string = pin_type_name(pin_type).to_string();
    pin(project_id, pin_type, path, options)?;

    Ok(ProjectPinOutput {
        action: "add".to_string(),
        project_id: project_id.to_string(),
        r#type: type_string.clone(),
        items: None,
        added: Some(ProjectPinChange {
            path: path.to_string(),
            r#type: type_string,
        }),
        removed: None,
        updated: None,
    })
}

pub fn remove_pin(project_id: &str, pin_type: PinType, path: &str) -> Result<ProjectPinOutput> {
    let type_string = pin_type_name(pin_type).to_string();
    unpin(project_id, pin_type, path)?;

    Ok(ProjectPinOutput {
        action: "remove".to_string(),
        project_id: project_id.to_string(),
        r#type: type_string.clone(),
        items: None,
        added: None,
        removed: Some(ProjectPinChange {
            path: path.to_string(),
            r#type: type_string,
        }),
        updated: None,
    })
}

pub fn update_pin(
    project_id: &str,
    pin_type: PinType,
    path: &str,
    options: PinUpdateOptions,
) -> Result<ProjectPinOutput> {
    let type_string = pin_type_name(pin_type).to_string();
    let mut project = load(project_id)?;
    let updated = update_pin_in_project(&mut project, pin_type, path, options)?;
    save(&project)?;

    Ok(ProjectPinOutput {
        action: "update".to_string(),
        project_id: project_id.to_string(),
        r#type: type_string,
        items: None,
        added: None,
        removed: None,
        updated: Some(updated),
    })
}

pub fn rename_pin(
    project_id: &str,
    pin_type: PinType,
    old_path: &str,
    new_path: &str,
) -> Result<ProjectPinOutput> {
    let type_string = pin_type_name(pin_type).to_string();
    let mut project = load(project_id)?;
    let updated = rename_pin_in_project(&mut project, pin_type, old_path, new_path)?;
    save(&project)?;

    Ok(ProjectPinOutput {
        action: "rename".to_string(),
        project_id: project_id.to_string(),
        r#type: type_string.clone(),
        items: None,
        added: Some(ProjectPinChange {
            path: new_path.to_string(),
            r#type: type_string.clone(),
        }),
        removed: Some(ProjectPinChange {
            path: old_path.to_string(),
            r#type: type_string,
        }),
        updated: Some(updated),
    })
}

fn update_pin_in_project(
    project: &mut Project,
    pin_type: PinType,
    path: &str,
    options: PinUpdateOptions,
) -> Result<ProjectPinListItem> {
    if options.label.is_none() && options.tail_lines.is_none() {
        return Err(Error::validation_invalid_argument(
            "options",
            "Provide --label or --tail to update a pin",
            Some(project.id.clone()),
            Some(vec![path.to_string()]),
        ));
    }

    match pin_type {
        PinType::File => {
            if options.tail_lines.is_some() {
                return Err(Error::validation_invalid_argument(
                    "tail",
                    "Tail lines can only be updated for log pins",
                    Some(project.id.clone()),
                    Some(vec![path.to_string()]),
                ));
            }

            let index = project
                .remote_files
                .pinned_files
                .iter()
                .position(|file| file.path == path)
                .ok_or_else(|| pin_not_found(project, pin_type, path))?;
            let file = &mut project.remote_files.pinned_files[index];

            if let Some(label) = options.label {
                file.label = Some(label);
            }
            Ok(ProjectPinListItem {
                path: file.path.clone(),
                label: file.label.clone(),
                display_name: file.display_name().to_string(),
                tail_lines: None,
            })
        }
        PinType::Log => {
            let index = project
                .remote_logs
                .pinned_logs
                .iter()
                .position(|log| log.path == path)
                .ok_or_else(|| pin_not_found(project, pin_type, path))?;
            let log = &mut project.remote_logs.pinned_logs[index];

            if let Some(label) = options.label {
                log.label = Some(label);
            }
            if let Some(tail_lines) = options.tail_lines {
                log.tail_lines = tail_lines;
            }
            Ok(ProjectPinListItem {
                path: log.path.clone(),
                label: log.label.clone(),
                display_name: log.display_name().to_string(),
                tail_lines: Some(log.tail_lines),
            })
        }
    }
}

fn rename_pin_in_project(
    project: &mut Project,
    pin_type: PinType,
    old_path: &str,
    new_path: &str,
) -> Result<ProjectPinListItem> {
    if old_path == new_path {
        return Err(Error::validation_invalid_argument(
            "new_path",
            "New pin path must differ from the current path",
            Some(project.id.clone()),
            Some(vec![old_path.to_string()]),
        ));
    }

    match pin_type {
        PinType::File => {
            if project
                .remote_files
                .pinned_files
                .iter()
                .any(|file| file.path == new_path)
            {
                return Err(Error::validation_invalid_argument(
                    "new_path",
                    "File is already pinned",
                    Some(project.id.clone()),
                    Some(vec![new_path.to_string()]),
                ));
            }

            let index = project
                .remote_files
                .pinned_files
                .iter()
                .position(|file| file.path == old_path)
                .ok_or_else(|| pin_not_found(project, pin_type, old_path))?;
            project.remote_files.pinned_files[index].path = new_path.to_string();
            let file = &project.remote_files.pinned_files[index];

            Ok(ProjectPinListItem {
                path: file.path.clone(),
                label: file.label.clone(),
                display_name: file.display_name().to_string(),
                tail_lines: None,
            })
        }
        PinType::Log => {
            if project
                .remote_logs
                .pinned_logs
                .iter()
                .any(|log| log.path == new_path)
            {
                return Err(Error::validation_invalid_argument(
                    "new_path",
                    "Log is already pinned",
                    Some(project.id.clone()),
                    Some(vec![new_path.to_string()]),
                ));
            }

            let index = project
                .remote_logs
                .pinned_logs
                .iter()
                .position(|log| log.path == old_path)
                .ok_or_else(|| pin_not_found(project, pin_type, old_path))?;
            project.remote_logs.pinned_logs[index].path = new_path.to_string();
            let log = &project.remote_logs.pinned_logs[index];

            Ok(ProjectPinListItem {
                path: log.path.clone(),
                label: log.label.clone(),
                display_name: log.display_name().to_string(),
                tail_lines: Some(log.tail_lines),
            })
        }
    }
}

fn pin_not_found(project: &Project, pin_type: PinType, path: &str) -> Error {
    Error::validation_invalid_argument(
        "path",
        format!("{} is not pinned", pin_type_name(pin_type)),
        Some(project.id.clone()),
        Some(vec![path.to_string()]),
    )
}

fn pin_type_name(pin_type: PinType) -> &'static str {
    match pin_type {
        PinType::File => "file",
        PinType::Log => "log",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{PinnedRemoteFile, PinnedRemoteLog, RemoteFileConfig, RemoteLogConfig};

    fn project() -> Project {
        Project {
            id: "site".to_string(),
            remote_files: RemoteFileConfig {
                pinned_files: vec![PinnedRemoteFile {
                    path: "wp-config.php".to_string(),
                    label: Some("Config".to_string()),
                }],
            },
            remote_logs: RemoteLogConfig {
                pinned_logs: vec![
                    PinnedRemoteLog {
                        path: "logs/php.log".to_string(),
                        label: Some("PHP".to_string()),
                        tail_lines: 100,
                    },
                    PinnedRemoteLog {
                        path: "logs/nginx.log".to_string(),
                        label: Some("Nginx".to_string()),
                        tail_lines: 50,
                    },
                ],
            },
            ..Default::default()
        }
    }

    #[test]
    fn update_log_pin_changes_tail_lines() {
        let mut project = project();

        let updated = update_pin_in_project(
            &mut project,
            PinType::Log,
            "logs/php.log",
            PinUpdateOptions {
                label: Some("PHP error log".to_string()),
                tail_lines: Some(250),
            },
        )
        .expect("update log pin");

        assert_eq!(updated.path, "logs/php.log");
        assert_eq!(updated.label.as_deref(), Some("PHP error log"));
        assert_eq!(updated.tail_lines, Some(250));
        assert_eq!(project.remote_logs.pinned_logs[0].tail_lines, 250);
    }

    #[test]
    fn tail_only_log_update_preserves_label() {
        let mut project = project();

        let updated = update_pin_in_project(
            &mut project,
            PinType::Log,
            "logs/php.log",
            PinUpdateOptions {
                label: None,
                tail_lines: Some(25),
            },
        )
        .expect("update log tail");

        assert_eq!(updated.label.as_deref(), Some("PHP"));
        assert_eq!(updated.tail_lines, Some(25));
    }

    #[test]
    fn rename_file_pin_changes_path() {
        let mut project = project();

        let updated = rename_pin_in_project(
            &mut project,
            PinType::File,
            "wp-config.php",
            "wp-config-local.php",
        )
        .expect("rename file pin");

        assert_eq!(updated.path, "wp-config-local.php");
        assert_eq!(updated.label.as_deref(), Some("Config"));
        assert_eq!(
            project.remote_files.pinned_files[0].path,
            "wp-config-local.php"
        );
    }

    #[test]
    fn failed_log_update_leaves_previous_state_intact() {
        let mut project = project();
        let before = project.clone();

        let err = update_pin_in_project(
            &mut project,
            PinType::Log,
            "logs/missing.log",
            PinUpdateOptions {
                label: Some("Missing".to_string()),
                tail_lines: Some(500),
            },
        )
        .expect_err("missing log update should fail");

        assert!(err.message.contains("log is not pinned"));
        assert_eq!(
            project.remote_logs.pinned_logs,
            before.remote_logs.pinned_logs
        );
    }

    #[test]
    fn failed_file_rename_leaves_previous_state_intact() {
        let mut project = project();
        project.remote_files.pinned_files.push(PinnedRemoteFile {
            path: "index.php".to_string(),
            label: None,
        });
        let before = project.clone();

        let err = rename_pin_in_project(&mut project, PinType::File, "wp-config.php", "index.php")
            .expect_err("duplicate rename should fail");

        assert!(err.message.contains("File is already pinned"));
        assert_eq!(
            project.remote_files.pinned_files,
            before.remote_files.pinned_files
        );
    }
}
