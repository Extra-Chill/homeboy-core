/// Environment variable names for module execution context.
/// Modules receive these variables when executed via `homeboy module run`.
/// Version of the exec context protocol. Modules can check this for compatibility.
pub const VERSION: &str = "HOMEBOY_EXEC_CONTEXT_VERSION";
/// ID of the module being executed.
pub const MODULE_ID: &str = "HOMEBOY_MODULE_ID";
/// JSON-serialized settings (merged from app, project, and component levels).
pub const SETTINGS_JSON: &str = "HOMEBOY_SETTINGS_JSON";
/// Project ID (only set when module requires project context).
pub const PROJECT_ID: &str = "HOMEBOY_PROJECT_ID";
/// Component ID (only set when module requires component context).
pub const COMPONENT_ID: &str = "HOMEBOY_COMPONENT_ID";
/// Filesystem path to the module directory.
pub const MODULE_PATH: &str = "HOMEBOY_MODULE_PATH";
/// Filesystem path to the project directory (base_path).
pub const PROJECT_PATH: &str = "HOMEBOY_PROJECT_PATH";
/// Filesystem path to the component directory.
pub const COMPONENT_PATH: &str = "HOMEBOY_COMPONENT_PATH";

/// Run only specific steps (comma-separated). Scripts should skip steps not in this list.
pub const STEP: &str = "HOMEBOY_STEP";
/// Skip specific steps (comma-separated). Scripts should skip steps in this list.
pub const SKIP: &str = "HOMEBOY_SKIP";

/// Current version of the exec context protocol.
pub const CURRENT_VERSION: &str = "2";
