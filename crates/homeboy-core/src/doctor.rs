use crate::config::{
    AppConfig, AppPaths, ComponentConfiguration, ProjectConfiguration, ServerConfig,
};
use crate::module::ModuleManifest;
use serde::Serialize;
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DoctorSeverity {
    Info,
    Warning,
    Error,
}

impl DoctorSeverity {
    fn sort_key(&self) -> u8 {
        match self {
            DoctorSeverity::Error => 0,
            DoctorSeverity::Warning => 1,
            DoctorSeverity::Info => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorIssue {
    pub severity: DoctorSeverity,
    pub code: String,
    pub message: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pointer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorSummary {
    pub files_scanned: usize,
    pub issues: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub command: String,
    pub summary: DoctorSummary,
    pub issues: Vec<DoctorIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorScope {
    All,
    App,
    Projects,
    Servers,
    Components,
    Modules,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn {
    Error,
    Warning,
}

pub struct Doctor;

impl Doctor {
    pub fn scan(scope: DoctorScope) -> crate::Result<DoctorScanResult> {
        let mut scanner = Scanner::new();
        scanner.scan(scope);
        Ok(scanner.finish())
    }

    pub fn scan_file(path: &Path) -> crate::Result<DoctorScanResult> {
        let mut scanner = Scanner::new();
        scanner.scan_file(path);
        Ok(scanner.finish())
    }

    pub fn exit_code(result: &DoctorScanResult, fail_on: FailOn) -> i32 {
        let has_errors = result
            .report
            .issues
            .iter()
            .any(|i| i.severity == DoctorSeverity::Error);
        if has_errors {
            return 1;
        }

        if fail_on == FailOn::Warning {
            let has_warnings = result
                .report
                .issues
                .iter()
                .any(|i| i.severity == DoctorSeverity::Warning);
            if has_warnings {
                return 1;
            }
        }

        0
    }
}

pub struct DoctorScanResult {
    pub report: DoctorReport,
    pub files_scanned: Vec<String>,
}

struct Scanner {
    issues: Vec<DoctorIssue>,
    files_scanned: Vec<String>,
    app_config: Option<AppConfig>,
    projects: BTreeMap<String, ProjectConfiguration>,
    servers: BTreeMap<String, ServerConfig>,
    components: BTreeMap<String, ComponentConfiguration>,
    modules: BTreeMap<String, ModuleManifest>,
}

impl Scanner {
    fn new() -> Self {
        Self {
            issues: Vec::new(),
            files_scanned: Vec::new(),
            app_config: None,
            projects: BTreeMap::new(),
            servers: BTreeMap::new(),
            components: BTreeMap::new(),
            modules: BTreeMap::new(),
        }
    }

    fn scan(&mut self, scope: DoctorScope) {
        match scope {
            DoctorScope::All => {
                self.scan(DoctorScope::App);
                self.scan(DoctorScope::Projects);
                self.scan(DoctorScope::Servers);
                self.scan(DoctorScope::Components);
                self.scan(DoctorScope::Modules);
                self.validate_cross_refs();
            }
            DoctorScope::App => {
                let path = AppPaths::config();
                if path.exists() {
                    self.scan_app_config(&path);
                }
            }
            DoctorScope::Projects => self.scan_dir_json(AppPaths::projects(), FileKind::Project),
            DoctorScope::Servers => self.scan_dir_json(AppPaths::servers(), FileKind::Server),
            DoctorScope::Components => {
                self.scan_dir_json(AppPaths::components(), FileKind::Component)
            }
            DoctorScope::Modules => self.scan_modules(),
        }
    }

    fn scan_file(&mut self, path: &Path) {
        if let Some(kind) = classify_file(path) {
            match kind {
                FileKind::App => self.scan_app_config(path),
                FileKind::Project => {
                    let path_buf = path.to_path_buf();
                    if let Some((raw, typed)) = self.read_typed_json_file::<ProjectConfiguration>(
                        &path_buf,
                        "ProjectConfiguration",
                    ) {
                        self.on_project_file(&path_buf, raw, typed);
                    }
                }
                FileKind::Server => {
                    let path_buf = path.to_path_buf();
                    if let Some((raw, typed)) =
                        self.read_typed_json_file::<ServerConfig>(&path_buf, "ServerConfig")
                    {
                        self.on_server_file(&path_buf, raw, typed);
                    }
                }
                FileKind::Component => {
                    let path_buf = path.to_path_buf();
                    if let Some((raw, typed)) = self.read_typed_json_file::<ComponentConfiguration>(
                        &path_buf,
                        "ComponentConfiguration",
                    ) {
                        self.on_component_file(&path_buf, raw, typed);
                    }
                }
                FileKind::ModuleManifest => {
                    let path_buf = path.to_path_buf();
                    if let Some((raw, typed)) =
                        self.read_typed_json_file::<ModuleManifest>(&path_buf, "ModuleManifest")
                    {
                        self.on_module_manifest_file(&path_buf, raw, typed);
                    }
                }
            }
            self.validate_cross_refs();
            return;
        }

        self.scan_generic_json(path);
    }

    fn scan_app_config(&mut self, path: &Path) {
        let path_buf = path.to_path_buf();
        if let Some((raw, typed)) = self.read_typed_json_file::<AppConfig>(&path_buf, "AppConfig") {
            self.emit_unknown_keys(&path_buf, "AppConfig", &raw, &typed);
            self.app_config = Some(typed);
        }
    }

    fn scan_modules(&mut self) {
        let modules_dir = AppPaths::modules();
        if !modules_dir.exists() {
            return;
        }

        let Ok(entries) = fs::read_dir(&modules_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("module.json");
            if !manifest_path.exists() {
                continue;
            }

            if let Some((raw, typed)) =
                self.read_typed_json_file::<ModuleManifest>(&manifest_path, "ModuleManifest")
            {
                self.on_module_manifest_file(&manifest_path, raw, typed);
            }
        }
    }

    fn scan_dir_json(&mut self, dir: PathBuf, kind: FileKind) {
        if !dir.exists() {
            return;
        }

        let Ok(entries) = fs::read_dir(&dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().is_some_and(|ext| ext == "json") {
                continue;
            }

            match kind {
                FileKind::Project => {
                    if let Some((raw, typed)) = self
                        .read_typed_json_file::<ProjectConfiguration>(&path, "ProjectConfiguration")
                    {
                        self.on_project_file(&path, raw, typed);
                    }
                }
                FileKind::Server => {
                    if let Some((raw, typed)) =
                        self.read_typed_json_file::<ServerConfig>(&path, "ServerConfig")
                    {
                        self.on_server_file(&path, raw, typed);
                    }
                }
                FileKind::Component => {
                    if let Some((raw, typed)) = self.read_typed_json_file::<ComponentConfiguration>(
                        &path,
                        "ComponentConfiguration",
                    ) {
                        self.on_component_file(&path, raw, typed);
                    }
                }
                _ => {}
            }
        }
    }

    fn scan_generic_json(&mut self, path: &Path) {
        self.track_file(path);
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) => {
                self.push_issue(
                    DoctorSeverity::Error,
                    "IO_READ_ERROR",
                    "Failed to read file",
                    path,
                    None,
                    Some(serde_json::json!({"error": err.to_string()})),
                );
                return;
            }
        };

        if let Err(err) = serde_json::from_str::<Value>(&content) {
            self.push_issue(
                DoctorSeverity::Error,
                "JSON_PARSE_ERROR",
                "Malformed JSON",
                path,
                None,
                Some(serde_json::json!({"error": err.to_string()})),
            );
        }
    }

    fn read_typed_json_file<T>(&mut self, path: &Path, expected: &str) -> Option<(Value, T)>
    where
        T: serde::de::DeserializeOwned,
    {
        self.track_file(path);
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(err) => {
                self.push_issue(
                    DoctorSeverity::Error,
                    "IO_READ_ERROR",
                    "Failed to read file",
                    path,
                    None,
                    Some(serde_json::json!({"error": err.to_string()})),
                );
                return None;
            }
        };

        let raw: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(err) => {
                self.push_issue(
                    DoctorSeverity::Error,
                    "JSON_PARSE_ERROR",
                    "Malformed JSON",
                    path,
                    None,
                    Some(serde_json::json!({"error": err.to_string()})),
                );
                return None;
            }
        };

        let typed: T = match serde_json::from_value(raw.clone()) {
            Ok(v) => v,
            Err(err) => {
                self.push_issue(
                    DoctorSeverity::Error,
                    "SCHEMA_DESERIALIZE_ERROR",
                    &format!("JSON does not match expected schema: {expected}"),
                    path,
                    None,
                    Some(serde_json::json!({"error": err.to_string()})),
                );
                return None;
            }
        };

        Some((raw, typed))
    }

    fn on_project_file(&mut self, path: &Path, raw: Value, project: ProjectConfiguration) {
        self.emit_unknown_keys(path, "ProjectConfiguration", &raw, &project);
        let id = file_stem_id(path);

        if project.name.trim().is_empty() {
            self.push_issue(
                DoctorSeverity::Error,
                "INVALID_VALUE",
                "Project name is empty",
                path,
                Some("/name".to_string()),
                None,
            );
        }
        if project.domain.trim().is_empty() {
            self.push_issue(
                DoctorSeverity::Error,
                "INVALID_VALUE",
                "Project domain is empty",
                path,
                Some("/domain".to_string()),
                None,
            );
        }
        if project.project_type.trim().is_empty() {
            self.push_issue(
                DoctorSeverity::Error,
                "INVALID_VALUE",
                "Project projectType is empty",
                path,
                Some("/projectType".to_string()),
                None,
            );
        }
        if let Some(prefix) = project.table_prefix.as_deref() {
            if !prefix.is_empty() && !prefix.ends_with('_') {
                self.push_issue(
                    DoctorSeverity::Warning,
                    "SUSPICIOUS_VALUE",
                    "WordPress tablePrefix usually ends with '_'",
                    path,
                    Some("/tablePrefix".to_string()),
                    Some(serde_json::json!({"value": prefix})),
                );
            }
        }

        self.projects.insert(id, project);
    }

    fn on_server_file(&mut self, path: &Path, raw: Value, server: ServerConfig) {
        self.emit_unknown_keys(path, "ServerConfig", &raw, &server);
        if !server.is_valid() {
            self.push_issue(
                DoctorSeverity::Error,
                "INVALID_VALUE",
                "Server must have non-empty host and user",
                path,
                None,
                Some(serde_json::json!({"host": server.host, "user": server.user})),
            );
        }

        let id = file_stem_id(path);
        self.servers.insert(id, server);
    }

    fn on_component_file(&mut self, path: &Path, raw: Value, component: ComponentConfiguration) {
        self.emit_unknown_keys(path, "ComponentConfiguration", &raw, &component);
        let id = file_stem_id(path);
        self.components.insert(id, component);
    }

    fn on_module_manifest_file(&mut self, path: &Path, raw: Value, mut manifest: ModuleManifest) {
        self.emit_unknown_keys(path, "ModuleManifest", &raw, &manifest);
        manifest.module_path = Some(
            path.parent()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .to_string(),
        );

        let missing = [
            ("id", manifest.id.trim().is_empty()),
            ("name", manifest.name.trim().is_empty()),
            ("version", manifest.version.trim().is_empty()),
            ("icon", manifest.icon.trim().is_empty()),
            ("description", manifest.description.trim().is_empty()),
            ("author", manifest.author.trim().is_empty()),
        ]
        .into_iter()
        .filter_map(|(field, is_bad)| if is_bad { Some(field) } else { None })
        .collect::<Vec<_>>();

        if !missing.is_empty() {
            self.push_issue(
                DoctorSeverity::Error,
                "INVALID_VALUE",
                "Module manifest has empty required fields",
                path,
                None,
                Some(serde_json::json!({"fields": missing})),
            );
        }

        self.modules.insert(manifest.id.clone(), manifest);
    }

    fn emit_unknown_keys<T: serde::Serialize>(
        &mut self,
        path: &Path,
        schema: &str,
        raw: &Value,
        typed: &T,
    ) {
        let Some(raw_obj) = raw.as_object() else {
            self.push_issue(
                DoctorSeverity::Error,
                "SCHEMA_TYPE_ERROR",
                &format!("Expected JSON object for {schema}"),
                path,
                None,
                None,
            );
            return;
        };

        let typed_value = match serde_json::to_value(typed) {
            Ok(v) => v,
            Err(err) => {
                self.push_issue(
                    DoctorSeverity::Error,
                    "INTERNAL_SERIALIZE_ERROR",
                    "Failed to serialize typed config for comparison",
                    path,
                    None,
                    Some(serde_json::json!({"error": err.to_string()})),
                );
                return;
            }
        };

        let Some(typed_obj) = typed_value.as_object() else {
            return;
        };

        let raw_keys: BTreeSet<&String> = raw_obj.keys().collect();
        let typed_keys: BTreeSet<&String> = typed_obj.keys().collect();

        let unknown: Vec<String> = raw_keys
            .difference(&typed_keys)
            .map(|s| (*s).clone())
            .collect();

        if !unknown.is_empty() {
            self.push_issue(
                DoctorSeverity::Warning,
                "UNKNOWN_KEYS",
                "File contains keys not in current schema",
                path,
                None,
                Some(serde_json::json!({"schema": schema, "keys": unknown})),
            );
        }
    }

    fn validate_cross_refs(&mut self) {
        let mut extra_issues = Vec::new();

        if let Some(app) = &self.app_config {
            if let Some(active) = app.active_project_id.as_deref() {
                if !self.projects.contains_key(active) {
                    extra_issues.push(DoctorIssue {
                        severity: DoctorSeverity::Error,
                        code: "BROKEN_REFERENCE".to_string(),
                        message: "activeProjectId references missing project".to_string(),
                        file: AppPaths::config().to_string_lossy().to_string(),
                        pointer: Some("/activeProjectId".to_string()),
                        details: Some(serde_json::json!({"id": active})),
                    });
                }
            }
        }

        for (project_id, project) in &self.projects {
            if let Some(server_id) = project.server_id.as_deref() {
                if !self.servers.contains_key(server_id) {
                    extra_issues.push(DoctorIssue {
                        severity: DoctorSeverity::Warning,
                        code: "BROKEN_REFERENCE".to_string(),
                        message: "project.serverId references missing server".to_string(),
                        file: AppPaths::project(project_id).to_string_lossy().to_string(),
                        pointer: Some("/serverId".to_string()),
                        details: Some(serde_json::json!({"id": server_id})),
                    });
                }
            }

            for component_id in &project.component_ids {
                if !self.components.contains_key(component_id) {
                    extra_issues.push(DoctorIssue {
                        severity: DoctorSeverity::Warning,
                        code: "BROKEN_REFERENCE".to_string(),
                        message: "project.componentIds contains missing component".to_string(),
                        file: AppPaths::project(project_id).to_string_lossy().to_string(),
                        pointer: Some("/componentIds".to_string()),
                        details: Some(serde_json::json!({"id": component_id})),
                    });
                }
            }
        }

        for (module_id, manifest) in &self.modules {
            let Some(requires) = &manifest.requires else {
                continue;
            };
            let Some(required_components) = &requires.components else {
                continue;
            };

            for component_id in required_components {
                if self.components.contains_key(component_id) {
                    continue;
                }
                let module_dir = manifest.module_path.as_deref().unwrap_or(module_id);
                extra_issues.push(DoctorIssue {
                    severity: DoctorSeverity::Warning,
                    code: "BROKEN_REFERENCE".to_string(),
                    message: "module requires missing component".to_string(),
                    file: PathBuf::from(module_dir)
                        .join("module.json")
                        .to_string_lossy()
                        .to_string(),
                    pointer: Some("/requires/components".to_string()),
                    details: Some(serde_json::json!({"id": component_id})),
                });
            }
        }

        self.issues.extend(extra_issues);
    }

    fn track_file(&mut self, path: &Path) {
        self.files_scanned.push(path.to_string_lossy().to_string());
    }

    fn push_issue(
        &mut self,
        severity: DoctorSeverity,
        code: &str,
        message: &str,
        file: &Path,
        pointer: Option<String>,
        details: Option<Value>,
    ) {
        self.issues.push(DoctorIssue {
            severity,
            code: code.to_string(),
            message: message.to_string(),
            file: file.to_string_lossy().to_string(),
            pointer,
            details,
        });
    }

    fn finish(mut self) -> DoctorScanResult {
        let mut counts = BTreeMap::new();
        counts.insert(
            "error".to_string(),
            self.issues
                .iter()
                .filter(|i| i.severity == DoctorSeverity::Error)
                .count(),
        );
        counts.insert(
            "warning".to_string(),
            self.issues
                .iter()
                .filter(|i| i.severity == DoctorSeverity::Warning)
                .count(),
        );
        counts.insert(
            "info".to_string(),
            self.issues
                .iter()
                .filter(|i| i.severity == DoctorSeverity::Info)
                .count(),
        );

        self.issues.sort_by(|a, b| {
            let by_severity = a.severity.sort_key().cmp(&b.severity.sort_key());
            if by_severity != Ordering::Equal {
                return by_severity;
            }
            let by_code = a.code.cmp(&b.code);
            if by_code != Ordering::Equal {
                return by_code;
            }
            a.file.cmp(&b.file)
        });

        DoctorScanResult {
            report: DoctorReport {
                command: "doctor".to_string(),
                summary: DoctorSummary {
                    files_scanned: self.files_scanned.len(),
                    issues: counts,
                },
                issues: self.issues,
            },
            files_scanned: self.files_scanned,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    App,
    Project,
    Server,
    Component,
    ModuleManifest,
}

fn classify_file(path: &Path) -> Option<FileKind> {
    if path.file_name().is_some_and(|n| n == "config.json") {
        return Some(FileKind::App);
    }

    let Some(parent) = path.parent().and_then(|p| p.file_name()) else {
        return None;
    };

    match parent.to_string_lossy().as_ref() {
        "projects" => Some(FileKind::Project),
        "servers" => Some(FileKind::Server),
        "components" => Some(FileKind::Component),
        _ => {
            if path.file_name().is_some_and(|n| n == "module.json") {
                Some(FileKind::ModuleManifest)
            } else {
                None
            }
        }
    }
}

fn file_stem_id(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_are_detected() {
        let mut scanner = Scanner::new();
        let raw = serde_json::json!({
            "activeProjectId": "abc",
            "unknownField": 123
        });
        let typed = AppConfig {
            active_project_id: Some("abc".to_string()),
            ..Default::default()
        };
        let path = Path::new("/tmp/config.json");
        scanner.emit_unknown_keys(path, "AppConfig", &raw, &typed);

        assert!(scanner
            .issues
            .iter()
            .any(|i| i.code == "UNKNOWN_KEYS" && i.severity == DoctorSeverity::Warning));
    }

    #[test]
    fn broken_active_project_is_error() {
        let mut scanner = Scanner::new();
        scanner.app_config = Some(AppConfig {
            active_project_id: Some("missing".to_string()),
            ..Default::default()
        });
        scanner.validate_cross_refs();

        assert!(scanner.issues.iter().any(|i| {
            i.code == "BROKEN_REFERENCE"
                && i.severity == DoctorSeverity::Error
                && i.pointer.as_deref() == Some("/activeProjectId")
        }));
    }
}
