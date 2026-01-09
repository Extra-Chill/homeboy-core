use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Server not found: {0}")]
    ServerNotFound(String),

    #[error("Component not found: {0}")]
    ComponentNotFound(String),

    #[error("No active project set")]
    NoActiveProject,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SSH error: {0}")]
    Ssh(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Config(_) => "CONFIG_ERROR",
            Error::ProjectNotFound(_) => "PROJECT_NOT_FOUND",
            Error::ServerNotFound(_) => "SERVER_NOT_FOUND",
            Error::ComponentNotFound(_) => "COMPONENT_NOT_FOUND",
            Error::NoActiveProject => "NO_ACTIVE_PROJECT",
            Error::Io(_) => "IO_ERROR",
            Error::Json(_) => "JSON_ERROR",
            Error::Ssh(_) => "SSH_ERROR",
            Error::Other(_) => "ERROR",
        }
    }
}
