use crate::config::{ConfigManager, ProjectConfiguration, ServerConfig};
use crate::{Error, Result};

pub struct ProjectServerContext {
    pub project: ProjectConfiguration,
    pub server_id: String,
    pub server: ServerConfig,
}

pub enum ResolvedTarget {
    Project(ProjectServerContext),
    Server {
        server_id: String,
        server: ServerConfig,
    },
}

pub fn resolve_project_server(project_id: &str) -> Result<ProjectServerContext> {
    let project = ConfigManager::load_project(project_id)?;

    let server_id = project.server_id.clone().ok_or_else(|| {
        Error::Config(format!(
            "Server not configured for project '{}'",
            project_id
        ))
    })?;

    let server = ConfigManager::load_server(&server_id)
        .map_err(|_| Error::Config(format!("Server '{}' not found", server_id)))?;

    Ok(ProjectServerContext {
        project,
        server_id,
        server,
    })
}

pub fn require_project_base_path(
    project_id: &str,
    project: &ProjectConfiguration,
) -> Result<String> {
    Ok(project
        .base_path
        .clone()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| {
            Error::Config(format!(
                "Base path not configured for project '{}'",
                project_id
            ))
        })?)
}

pub fn resolve_project_server_with_base_path(
    project_id: &str,
) -> Result<(ProjectServerContext, String)> {
    let ctx = resolve_project_server(project_id)?;
    let base_path = require_project_base_path(project_id, &ctx.project)?;
    Ok((ctx, base_path))
}

pub fn resolve_project_or_server_id(id: &str) -> Result<ResolvedTarget> {
    if let Ok(ctx) = resolve_project_server(id) {
        return Ok(ResolvedTarget::Project(ctx));
    }

    let server = ConfigManager::load_server(id)
        .map_err(|_| Error::Config(format!("Server '{}' not found", id)))?;

    Ok(ResolvedTarget::Server {
        server_id: id.to_string(),
        server,
    })
}
