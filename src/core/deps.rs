use crate::component::{self, Component, DependencyStackEdge};
use crate::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyPackage {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_section: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub constraint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked_reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStatus {
    pub component_id: String,
    pub component_path: String,
    pub package_manager: String,
    pub packages: Vec<DependencyPackage>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyUpdateResult {
    pub component_id: String,
    pub component_path: String,
    pub package_manager: String,
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_constraint: Option<String>,
    pub command: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<DependencyPackage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<DependencyPackage>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackStatus {
    pub edge_count: usize,
    pub edges: Vec<DependencyStackEdgeStatus>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackEdgeStatus {
    pub declaring_component_id: String,
    pub upstream: String,
    pub downstream: String,
    pub package: String,
    pub update_command: String,
    pub post_update: Vec<String>,
    pub test: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackPlan {
    pub upstream: String,
    pub step_count: usize,
    pub steps: Vec<DependencyStackPlanStep>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackPlanStep {
    pub sequence: usize,
    pub declaring_component_id: String,
    pub upstream: String,
    pub downstream: String,
    pub downstream_path: String,
    pub package: String,
    pub update_command: String,
    pub post_update: Vec<String>,
    pub test: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackApplyResult {
    pub upstream: String,
    pub dry_run: bool,
    pub step_count: usize,
    pub steps: Vec<DependencyStackApplyStep>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackApplyStep {
    pub sequence: usize,
    pub downstream: String,
    pub command_results: Vec<DependencyStackCommandResult>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DependencyStackCommandResult {
    pub phase: String,
    pub command: String,
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerAction {
    Require { constraint: String },
    Update,
}

pub fn composer_command_args(package: &str, action: &ComposerAction) -> Vec<String> {
    match action {
        ComposerAction::Require { constraint } => vec![
            "require".to_string(),
            format!("{package}:{constraint}"),
            "--with-dependencies".to_string(),
            "--no-interaction".to_string(),
        ],
        ComposerAction::Update => vec![
            "update".to_string(),
            package.to_string(),
            "--with-dependencies".to_string(),
            "--no-interaction".to_string(),
        ],
    }
}

pub fn status(
    component_id: Option<&str>,
    path_override: Option<&str>,
    package_filter: Option<&str>,
) -> Result<DependencyStatus> {
    let (component, path) = resolve_component_path(component_id, path_override)?;
    composer_status(&component, &path, package_filter)
}

pub fn update(
    component_id: Option<&str>,
    path_override: Option<&str>,
    package: &str,
    constraint: Option<&str>,
) -> Result<DependencyUpdateResult> {
    let (component, path) = resolve_component_path(component_id, path_override)?;
    ensure_composer_component(&path)?;

    let before = package_snapshot(&path, package)?;
    let action = match constraint {
        Some(constraint) => ComposerAction::Require {
            constraint: constraint.to_string(),
        },
        None => ComposerAction::Update,
    };
    let args = composer_command_args(package, &action);
    let output = Command::new("composer")
        .args(&args)
        .current_dir(&path)
        .output()
        .map_err(|e| Error::internal_io(e.to_string(), Some("run composer".to_string())))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(Error::validation_invalid_argument(
            "composer",
            format!(
                "Composer command failed with status {}: {}",
                output.status,
                first_non_empty_line(&stderr)
                    .or_else(|| first_non_empty_line(&stdout))
                    .unwrap_or("no output")
            ),
            None,
            Some(vec![format!(
                "Run manually in {}: composer {}",
                path.display(),
                args.join(" ")
            )]),
        ));
    }

    let after = package_snapshot(&path, package)?;

    Ok(DependencyUpdateResult {
        component_id: component.id,
        component_path: path.display().to_string(),
        package_manager: "composer".to_string(),
        package: package.to_string(),
        requested_constraint: constraint.map(str::to_string),
        command: std::iter::once("composer".to_string())
            .chain(args)
            .collect(),
        before,
        after,
        stdout,
        stderr,
    })
}

pub fn stack_status() -> Result<DependencyStackStatus> {
    let mut edges = Vec::new();
    for component in component::list()? {
        for edge in &component.dependency_stack {
            edges.push(edge_status(&component, edge));
        }
    }

    edges.sort_by(|a, b| {
        a.upstream
            .cmp(&b.upstream)
            .then_with(|| a.downstream.cmp(&b.downstream))
            .then_with(|| a.package.cmp(&b.package))
    });

    Ok(DependencyStackStatus {
        edge_count: edges.len(),
        edges,
    })
}

pub fn stack_plan(upstream: &str) -> Result<DependencyStackPlan> {
    let components = component::list()?;
    stack_plan_from_components(upstream, &components)
}

pub fn stack_apply(upstream: &str, dry_run: bool) -> Result<DependencyStackApplyResult> {
    let plan = stack_plan(upstream)?;
    let mut steps = Vec::new();

    for step in &plan.steps {
        let mut command_results = Vec::new();
        command_results.push(run_stack_command(
            "update",
            &step.update_command,
            &step.downstream_path,
            dry_run,
        )?);
        for command in &step.post_update {
            command_results.push(run_stack_command(
                "post_update",
                command,
                &step.downstream_path,
                dry_run,
            )?);
        }
        for command in &step.test {
            command_results.push(run_stack_command(
                "test",
                command,
                &step.downstream_path,
                dry_run,
            )?);
        }
        steps.push(DependencyStackApplyStep {
            sequence: step.sequence,
            downstream: step.downstream.clone(),
            command_results,
        });
    }

    Ok(DependencyStackApplyResult {
        upstream: plan.upstream,
        dry_run,
        step_count: steps.len(),
        steps,
    })
}

pub fn stack_plan_from_components(
    upstream: &str,
    components: &[Component],
) -> Result<DependencyStackPlan> {
    let mut steps = Vec::new();
    let mut queue = vec![upstream.to_string()];
    let mut visited_edges = BTreeSet::new();
    let component_paths: BTreeMap<String, String> = components
        .iter()
        .map(|component| (component.id.clone(), component.local_path.clone()))
        .collect();

    while let Some(current_upstream) = queue.pop() {
        let mut matching_edges = Vec::new();
        for component in components {
            for edge in &component.dependency_stack {
                if edge.upstream == current_upstream {
                    matching_edges.push((component, edge));
                }
            }
        }
        matching_edges.sort_by(|(a_component, a_edge), (b_component, b_edge)| {
            a_edge
                .downstream
                .cmp(&b_edge.downstream)
                .then_with(|| a_edge.package.cmp(&b_edge.package))
                .then_with(|| a_component.id.cmp(&b_component.id))
        });

        for (component, edge) in matching_edges {
            let key = format!("{}>{}:{}", edge.upstream, edge.downstream, edge.package);
            if !visited_edges.insert(key) {
                continue;
            }
            let Some(downstream_path) = component_paths.get(&edge.downstream) else {
                return Err(Error::validation_invalid_argument(
                    "dependency_stack.downstream",
                    format!(
                        "Dependency stack edge {} -> {} references an unknown downstream component",
                        edge.upstream, edge.downstream
                    ),
                    Some(edge.downstream.clone()),
                    Some(vec![
                        "Add the downstream component to Homeboy inventory".to_string(),
                        "Or fix dependency_stack[].downstream in homeboy.json".to_string(),
                    ]),
                ));
            };
            steps.push(DependencyStackPlanStep {
                sequence: steps.len() + 1,
                declaring_component_id: component.id.clone(),
                upstream: edge.upstream.clone(),
                downstream: edge.downstream.clone(),
                downstream_path: downstream_path.clone(),
                package: edge.package.clone(),
                update_command: update_command(edge, downstream_path),
                post_update: edge.post_update.clone(),
                test: edge.test.clone(),
            });
            queue.push(edge.downstream.clone());
        }
    }

    Ok(DependencyStackPlan {
        upstream: upstream.to_string(),
        step_count: steps.len(),
        steps,
    })
}

fn edge_status(component: &Component, edge: &DependencyStackEdge) -> DependencyStackEdgeStatus {
    DependencyStackEdgeStatus {
        declaring_component_id: component.id.clone(),
        upstream: edge.upstream.clone(),
        downstream: edge.downstream.clone(),
        package: edge.package.clone(),
        update_command: update_command(edge, &component.local_path),
        post_update: edge.post_update.clone(),
        test: edge.test.clone(),
    }
}

fn update_command(edge: &DependencyStackEdge, downstream_path: &str) -> String {
    edge.update.clone().unwrap_or_else(|| {
        format!(
            "homeboy deps update {} --path {}",
            shell_word(&edge.package),
            shell_word(downstream_path)
        )
    })
}

fn run_stack_command(
    phase: &str,
    command: &str,
    cwd: &str,
    dry_run: bool,
) -> Result<DependencyStackCommandResult> {
    if dry_run {
        return Ok(DependencyStackCommandResult {
            phase: phase.to_string(),
            command: command.to_string(),
            skipped: true,
            status: None,
            stdout: String::new(),
            stderr: String::new(),
        });
    }

    let output = Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .output()
        .map_err(|e| Error::internal_io(e.to_string(), Some(format!("run {phase} command"))))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !output.status.success() {
        return Err(Error::validation_invalid_argument(
            "dependency_stack.command",
            format!(
                "Dependency stack {phase} command failed with status {}: {}",
                output.status,
                first_non_empty_line(&stderr)
                    .or_else(|| first_non_empty_line(&stdout))
                    .unwrap_or("no output")
            ),
            Some(command.to_string()),
            Some(vec![format!("Run manually in {cwd}: {command}")]),
        ));
    }

    Ok(DependencyStackCommandResult {
        phase: phase.to_string(),
        command: command.to_string(),
        skipped: false,
        status: output.status.code(),
        stdout,
        stderr,
    })
}

fn shell_word(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':' | '@'))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn resolve_component_path(
    component_id: Option<&str>,
    path_override: Option<&str>,
) -> Result<(Component, PathBuf)> {
    let component = component::resolve_effective(component_id, path_override, None)?;
    let path = PathBuf::from(shellexpand::tilde(&component.local_path).as_ref());

    if !path.exists() {
        return Err(Error::validation_invalid_argument(
            "component_path",
            format!(
                "Component '{}' path does not exist: {}",
                component.id,
                path.display()
            ),
            Some(component.id.clone()),
            None,
        ));
    }

    Ok((component, path))
}

fn ensure_composer_component(path: &Path) -> Result<()> {
    if path.join("composer.json").is_file() {
        return Ok(());
    }

    Err(Error::validation_invalid_argument(
        "package_manager",
        format!("No supported dependency manifest found in {}", path.display()),
        None,
        Some(vec![
            "Composer MVP requires composer.json at the component root".to_string(),
            "npm, Cargo, and other package managers are intentionally out of scope for this command".to_string(),
        ]),
    ))
}

fn composer_status(
    component: &Component,
    path: &Path,
    package_filter: Option<&str>,
) -> Result<DependencyStatus> {
    ensure_composer_component(path)?;
    let packages = read_composer_packages(path, package_filter)?;

    Ok(DependencyStatus {
        component_id: component.id.clone(),
        component_path: path.display().to_string(),
        package_manager: "composer".to_string(),
        packages,
    })
}

fn package_snapshot(path: &Path, package: &str) -> Result<Option<DependencyPackage>> {
    Ok(read_composer_packages(path, Some(package))?
        .into_iter()
        .next())
}

fn read_composer_packages(
    path: &Path,
    package_filter: Option<&str>,
) -> Result<Vec<DependencyPackage>> {
    let manifest = read_json_file(&path.join("composer.json"))?;
    let lock = read_optional_json_file(&path.join("composer.lock"))?;
    let mut direct = BTreeMap::new();

    collect_manifest_section(&manifest, "require", &mut direct);
    collect_manifest_section(&manifest, "require-dev", &mut direct);

    let locked = lock
        .as_ref()
        .map(collect_locked_packages)
        .unwrap_or_default();

    let mut names: BTreeSet<String> = direct.keys().cloned().collect();
    names.extend(locked.keys().cloned());

    let packages = names
        .into_iter()
        .filter(|name| package_filter.map(|filter| filter == name).unwrap_or(true))
        .map(|name| {
            let (manifest_section, constraint) = direct
                .get(&name)
                .cloned()
                .map(|(section, constraint)| (Some(section), Some(constraint)))
                .unwrap_or((None, None));
            let locked = locked.get(&name);
            DependencyPackage {
                name,
                manifest_section,
                constraint,
                locked_version: locked.and_then(|p| p.version.clone()),
                locked_reference: locked.and_then(|p| p.reference.clone()),
            }
        })
        .collect();

    Ok(packages)
}

fn collect_manifest_section(
    manifest: &Value,
    section: &str,
    direct: &mut BTreeMap<String, (String, String)>,
) {
    let Some(entries) = manifest.get(section).and_then(Value::as_object) else {
        return;
    };

    for (name, constraint) in entries {
        if name == "php" || name.starts_with("ext-") {
            continue;
        }
        if let Some(constraint) = constraint.as_str() {
            direct.insert(name.clone(), (section.to_string(), constraint.to_string()));
        }
    }
}

#[derive(Debug, Clone, Default)]
struct LockedPackage {
    version: Option<String>,
    reference: Option<String>,
}

fn collect_locked_packages(lock: &Value) -> BTreeMap<String, LockedPackage> {
    let mut packages = BTreeMap::new();

    for section in ["packages", "packages-dev"] {
        let Some(entries) = lock.get(section).and_then(Value::as_array) else {
            continue;
        };

        for entry in entries {
            let Some(name) = entry.get("name").and_then(Value::as_str) else {
                continue;
            };
            let version = entry
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string);
            let reference = entry
                .get("source")
                .and_then(|source| source.get("reference"))
                .or_else(|| entry.get("dist").and_then(|dist| dist.get("reference")))
                .and_then(Value::as_str)
                .map(str::to_string);

            packages.insert(name.to_string(), LockedPackage { version, reference });
        }
    }

    packages
}

fn read_json_file(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path)
        .map_err(|e| Error::internal_io(e.to_string(), Some(path.display().to_string())))?;
    serde_json::from_str(&raw)
        .map_err(|e| Error::validation_invalid_json(e, Some(path.display().to_string()), Some(raw)))
}

fn read_optional_json_file(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    read_json_file(path).map(Some)
}

fn first_non_empty_line(output: &str) -> Option<&str> {
    output.lines().find(|line| !line.trim().is_empty())
}
