use clap::{Args, Subcommand};
use homeboy::log_status;
use serde::Serialize;

use homeboy::component;
use homeboy::deploy::{self, DeployConfig};
use homeboy::fleet::{self, Fleet};
use homeboy::project::{self, Project};
use homeboy::server;
use homeboy::ssh::SshClient;
use homeboy::version;
use homeboy::EntityCrudOutput;

use super::{CmdResult, DynamicSetArgs};

#[derive(Args)]
pub struct FleetArgs {
    #[command(subcommand)]
    command: FleetCommand,
}

#[derive(Subcommand)]
enum FleetCommand {
    /// Create a new fleet
    Create {
        /// Fleet ID
        id: String,

        /// Project IDs to include (comma-separated or repeated)
        #[arg(long, short = 'p', value_delimiter = ',')]
        projects: Option<Vec<String>>,

        /// Description of the fleet
        #[arg(long, short = 'd')]
        description: Option<String>,
    },
    /// Display fleet configuration
    Show {
        /// Fleet ID
        id: String,
    },
    /// Update fleet configuration
    #[command(visible_aliases = ["edit", "merge"])]
    Set {
        #[command(flatten)]
        args: DynamicSetArgs,
    },
    /// Delete a fleet
    Delete {
        /// Fleet ID
        id: String,
    },
    /// List all fleets
    List,
    /// Add a project to a fleet
    Add {
        /// Fleet ID
        id: String,

        /// Project ID to add
        #[arg(long, short = 'p')]
        project: String,
    },
    /// Remove a project from a fleet
    Remove {
        /// Fleet ID
        id: String,

        /// Project ID to remove
        #[arg(long, short = 'p')]
        project: String,
    },
    /// Show projects in a fleet
    Projects {
        /// Fleet ID
        id: String,
    },
    /// Show component usage across a fleet
    Components {
        /// Fleet ID
        id: String,
    },
    /// Show live component versions and server health across a fleet (via SSH)
    Status {
        /// Fleet ID
        id: String,

        /// Use locally cached versions instead of live SSH check
        #[arg(long)]
        cached: bool,

        /// Show only server health metrics, skip component versions
        #[arg(long)]
        health_only: bool,
    },
    /// Check component drift across a fleet (compares local vs remote)
    Check {
        /// Fleet ID
        id: String,

        /// Only show components that need updates
        #[arg(long)]
        outdated: bool,
    },
    /// Run a command across all projects in a fleet via SSH
    Exec {
        /// Fleet ID
        id: String,

        /// Command to execute on each project's server
        #[arg(num_args = 0.., trailing_var_arg = true)]
        command: Vec<String>,

        /// Show what would execute without running anything
        #[arg(long)]
        check: bool,

        /// Reserved for future parallel mode. Currently all execution is serial.
        #[arg(long, hide = true)]
        serial: bool,
    },
    /// [DEPRECATED] Use 'homeboy deploy' instead. See issue #101.
    Sync {
        /// Fleet ID
        id: String,

        /// Sync only specific categories (repeatable)
        #[arg(long, short = 'c', value_delimiter = ',')]
        category: Option<Vec<String>>,

        /// Show what would be synced without doing it
        #[arg(long)]
        dry_run: bool,

        /// Override leader server (defaults to fleet-sync.json config)
        #[arg(long)]
        leader: Option<String>,
    },
}

/// Entity-specific fields for fleet commands.
#[derive(Debug, Default, Serialize)]
pub struct FleetExtra {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<Vec<Project>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub components: Option<std::collections::HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Vec<FleetProjectStatus>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<Vec<FleetProjectCheck>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<FleetCheckSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<Vec<FleetExecProjectResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec_summary: Option<FleetExecSummary>,
}

pub type FleetOutput = EntityCrudOutput<Fleet, FleetExtra>;

#[derive(Debug, Default, Serialize)]
pub struct FleetExecProjectResult {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    pub command: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetExecSummary {
    pub total: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetProjectCheck {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub components: Vec<FleetComponentCheck>,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetComponentCheck {
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_version: Option<String>,
    pub status: String,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetCheckSummary {
    pub total_projects: u32,
    pub projects_checked: u32,
    pub projects_failed: u32,
    pub components_up_to_date: u32,
    pub components_needs_update: u32,
    pub components_unknown: u32,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetProjectStatus {
    pub project_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub components: Vec<FleetComponentStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<ServerHealth>,
}

#[derive(Debug, Default, Serialize)]
pub struct FleetComponentStatus {
    pub component_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Where the version was resolved from: "live" (SSH) or "cached" (local file)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_source: Option<String>,
}

// ============================================================================
// Server Health
// ============================================================================

#[derive(Debug, Default, Serialize, Clone)]
pub struct ServerHealth {
    /// Server uptime as a human-readable string (e.g. "10 days")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,
    /// Load averages: 1min, 5min, 15min
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<LoadAverage>,
    /// Disk usage for the primary filesystem
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<DiskUsage>,
    /// Memory usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryUsage>,
    /// Service statuses (only if project has services configured)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<ServiceStatus>,
    /// Warning messages for thresholds exceeded
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
    /// Number of CPU cores (for contextualizing load)
    pub cpu_cores: u32,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct DiskUsage {
    /// Used space in human-readable form (e.g. "36G")
    pub used: String,
    /// Total space in human-readable form (e.g. "150G")
    pub total: String,
    /// Usage percentage (0-100)
    pub percent: u32,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct MemoryUsage {
    /// Used memory in human-readable form (e.g. "2.2G")
    pub used: String,
    /// Total memory in human-readable form (e.g. "7.6G")
    pub total: String,
    /// Usage percentage (0-100)
    pub percent: u32,
}

#[derive(Debug, Default, Serialize, Clone)]
pub struct ServiceStatus {
    pub name: String,
    pub active: bool,
    /// Raw status string from systemctl (e.g. "active", "inactive", "failed")
    pub status: String,
}

pub fn run(args: FleetArgs, _global: &super::GlobalArgs) -> CmdResult<FleetOutput> {
    match args.command {
        FleetCommand::Create {
            id,
            projects,
            description,
        } => create(&id, projects.unwrap_or_default(), description),
        FleetCommand::Show { id } => show(&id),
        FleetCommand::Set { args } => set(args),
        FleetCommand::Delete { id } => delete(&id),
        FleetCommand::List => list(),
        FleetCommand::Add { id, project } => add(&id, &project),
        FleetCommand::Remove { id, project } => remove(&id, &project),
        FleetCommand::Projects { id } => projects(&id),
        FleetCommand::Components { id } => components(&id),
        FleetCommand::Status {
            id,
            cached,
            health_only,
        } => status(&id, cached, health_only),
        FleetCommand::Check { id, outdated } => check(&id, outdated),
        FleetCommand::Exec {
            id,
            command,
            check,
            serial: _,
        } => exec(&id, command, check),
        FleetCommand::Sync {
            id,
            category,
            dry_run,
            leader,
        } => sync(&id, category, dry_run, leader),
    }
}

fn create(
    id: &str,
    project_ids: Vec<String>,
    description: Option<String>,
) -> CmdResult<FleetOutput> {
    // Validate projects exist
    for pid in &project_ids {
        if !homeboy::project::exists(pid) {
            return Err(homeboy::Error::project_not_found(pid, vec![]));
        }
    }

    let mut new_fleet = Fleet::new(id.to_string(), project_ids);
    new_fleet.description = description;

    let json_spec = homeboy::config::to_json_string(&new_fleet)?;

    match fleet::create(&json_spec, false)? {
        homeboy::CreateOutput::Single(result) => Ok((
            FleetOutput {
                command: "fleet.create".to_string(),
                id: Some(result.id),
                entity: Some(result.entity),
                ..Default::default()
            },
            0,
        )),
        homeboy::CreateOutput::Bulk(_) => Err(homeboy::Error::internal_unexpected(
            "Unexpected bulk result for single fleet".to_string(),
        )),
    }
}

fn show(id: &str) -> CmdResult<FleetOutput> {
    let fl = fleet::load(id)?;

    Ok((
        FleetOutput {
            command: "fleet.show".to_string(),
            id: Some(id.to_string()),
            entity: Some(fl),
            ..Default::default()
        },
        0,
    ))
}

fn set(args: DynamicSetArgs) -> CmdResult<FleetOutput> {
    let merged = super::merge_dynamic_args(&args)?.ok_or_else(|| {
        homeboy::Error::validation_invalid_argument(
            "spec",
            "Provide JSON spec, --json flag, --base64 flag, or --key value flags",
            None,
            None,
        )
    })?;
    let (json_string, replace_fields) = super::finalize_set_spec(&merged, &args.replace)?;

    match fleet::merge(args.id.as_deref(), &json_string, &replace_fields)? {
        homeboy::MergeOutput::Single(result) => {
            let fl = fleet::load(&result.id)?;
            Ok((
                FleetOutput {
                    command: "fleet.set".to_string(),
                    id: Some(result.id),
                    entity: Some(fl),
                    updated_fields: result.updated_fields,
                    ..Default::default()
                },
                0,
            ))
        }
        homeboy::MergeOutput::Bulk(_) => Err(homeboy::Error::internal_unexpected(
            "Unexpected bulk result for single fleet".to_string(),
        )),
    }
}

fn delete(id: &str) -> CmdResult<FleetOutput> {
    fleet::delete(id)?;

    Ok((
        FleetOutput {
            command: "fleet.delete".to_string(),
            id: Some(id.to_string()),
            deleted: vec![id.to_string()],
            ..Default::default()
        },
        0,
    ))
}

fn list() -> CmdResult<FleetOutput> {
    let fleets = fleet::list()?;

    Ok((
        FleetOutput {
            command: "fleet.list".to_string(),
            entities: fleets,
            ..Default::default()
        },
        0,
    ))
}

fn add(fleet_id: &str, project_id: &str) -> CmdResult<FleetOutput> {
    let fl = fleet::add_project(fleet_id, project_id)?;

    Ok((
        FleetOutput {
            command: "fleet.add".to_string(),
            id: Some(fleet_id.to_string()),
            entity: Some(fl),
            updated_fields: vec!["project_ids".to_string()],
            ..Default::default()
        },
        0,
    ))
}

fn remove(fleet_id: &str, project_id: &str) -> CmdResult<FleetOutput> {
    let fl = fleet::remove_project(fleet_id, project_id)?;

    Ok((
        FleetOutput {
            command: "fleet.remove".to_string(),
            id: Some(fleet_id.to_string()),
            entity: Some(fl),
            updated_fields: vec!["project_ids".to_string()],
            ..Default::default()
        },
        0,
    ))
}

fn projects(id: &str) -> CmdResult<FleetOutput> {
    let projects = fleet::get_projects(id)?;

    Ok((
        FleetOutput {
            command: "fleet.projects".to_string(),
            id: Some(id.to_string()),
            extra: FleetExtra {
                projects: Some(projects),
                ..Default::default()
            },
            ..Default::default()
        },
        0,
    ))
}

fn components(id: &str) -> CmdResult<FleetOutput> {
    let components = fleet::component_usage(id)?;

    Ok((
        FleetOutput {
            command: "fleet.components".to_string(),
            id: Some(id.to_string()),
            extra: FleetExtra {
                components: Some(components),
                ..Default::default()
            },
            ..Default::default()
        },
        0,
    ))
}

// ============================================================================
// Health Collection
// ============================================================================

/// Collect server health metrics via a single SSH command.
///
/// Runs a compound shell command that outputs structured, delimited sections
/// for uptime, load, disk, memory, and optionally service statuses.
fn collect_server_health(client: &SshClient, services: &[String]) -> ServerHealth {
    // Build a single compound command to minimize SSH round-trips.
    // Each section is delimited by a marker line for reliable parsing.
    let mut cmd_parts = vec![
        "echo '---UPTIME---'".to_string(),
        "uptime -p 2>/dev/null || uptime".to_string(),
        "echo '---LOAD---'".to_string(),
        "cat /proc/loadavg 2>/dev/null".to_string(),
        "echo '---CPUS---'".to_string(),
        "nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo 2>/dev/null || echo 1".to_string(),
        "echo '---DISK---'".to_string(),
        "df -h / 2>/dev/null | tail -1".to_string(),
        "echo '---MEMORY---'".to_string(),
        "free -h 2>/dev/null | grep '^Mem:'".to_string(),
    ];

    if !services.is_empty() {
        cmd_parts.push("echo '---SERVICES---'".to_string());
        for svc in services {
            // Output: service_name:status
            cmd_parts.push(format!(
                "echo '{}:'$(systemctl is-active {} 2>/dev/null || echo 'unknown')",
                svc,
                shell_quote_service(svc)
            ));
        }
    }

    let compound_cmd = cmd_parts.join(" && ");
    let output = client.execute(&compound_cmd);

    if !output.success && output.stdout.is_empty() {
        // Total SSH failure — return empty health
        return ServerHealth::default();
    }

    parse_health_output(&output.stdout, services)
}

/// Quote a service name for safe use in shell commands.
fn shell_quote_service(name: &str) -> String {
    // Only allow alphanumeric, dash, dot, underscore, @ (systemd instance separator)
    if name
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '.' | '_' | '@'))
    {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "'\\''"))
    }
}

/// Parse the structured health output from the compound SSH command.
fn parse_health_output(output: &str, services: &[String]) -> ServerHealth {
    let mut health = ServerHealth::default();
    let mut current_section = "";

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        match trimmed {
            "---UPTIME---" => {
                current_section = "uptime";
                continue;
            }
            "---LOAD---" => {
                current_section = "load";
                continue;
            }
            "---CPUS---" => {
                current_section = "cpus";
                continue;
            }
            "---DISK---" => {
                current_section = "disk";
                continue;
            }
            "---MEMORY---" => {
                current_section = "memory";
                continue;
            }
            "---SERVICES---" => {
                current_section = "services";
                continue;
            }
            _ => {}
        }

        match current_section {
            "uptime" => {
                health.uptime = Some(parse_uptime(trimmed));
            }
            "load" => {
                health.load = parse_load_average(trimmed, health.load.as_ref());
            }
            "cpus" => {
                if let Some(ref mut load) = health.load {
                    if let Ok(cores) = trimmed.parse::<u32>() {
                        load.cpu_cores = cores;
                    }
                }
            }
            "disk" => {
                health.disk = parse_disk_usage(trimmed);
            }
            "memory" => {
                health.memory = parse_memory_usage(trimmed);
            }
            "services" if !services.is_empty() => {
                if let Some(svc) = parse_service_status(trimmed) {
                    health.services.push(svc);
                }
            }
            _ => {}
        }
    }

    // Generate warnings
    health.warnings = generate_warnings(&health);

    health
}

/// Parse `uptime -p` output (e.g. "up 10 days, 3 hours, 22 minutes") or
/// fallback `uptime` output.
fn parse_uptime(line: &str) -> String {
    // `uptime -p` produces "up 10 days, 3 hours, 22 minutes"
    if let Some(rest) = line.strip_prefix("up ") {
        return rest.to_string();
    }
    // Fallback: extract from full uptime line "... up 10 days, ..."
    if let Some(up_pos) = line.find("up ") {
        let after_up = &line[up_pos + 3..];
        // Find the end: typically before "user" or a load indicator
        if let Some(user_pos) = after_up.find("user") {
            let segment = after_up[..user_pos].trim_end_matches([',', ' ']);
            // Remove the trailing number (user count) — e.g. "10 days,  3"
            if let Some(last_comma) = segment.rfind(',') {
                return segment[..last_comma].trim().to_string();
            }
            return segment.to_string();
        }
        return after_up.trim().to_string();
    }
    line.to_string()
}

/// Parse /proc/loadavg (e.g. "0.02 0.08 0.08 1/234 12345").
fn parse_load_average(line: &str, existing: Option<&LoadAverage>) -> Option<LoadAverage> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 3 {
        Some(LoadAverage {
            one: parts[0].parse().unwrap_or(0.0),
            five: parts[1].parse().unwrap_or(0.0),
            fifteen: parts[2].parse().unwrap_or(0.0),
            cpu_cores: existing.map_or(1, |l| l.cpu_cores),
        })
    } else {
        None
    }
}

/// Parse `df -h /` output line (e.g. "/dev/sda1  150G  36G  114G  25% /").
fn parse_disk_usage(line: &str) -> Option<DiskUsage> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    // Standard df -h output: Filesystem Size Used Avail Use% Mounted
    if parts.len() >= 5 {
        let percent_str = parts[4].trim_end_matches('%');
        Some(DiskUsage {
            total: parts[1].to_string(),
            used: parts[2].to_string(),
            percent: percent_str.parse().unwrap_or(0),
        })
    } else {
        None
    }
}

/// Parse `free -h` Mem line (e.g. "Mem:  7.6Gi  2.2Gi  3.1Gi  ...").
fn parse_memory_usage(line: &str) -> Option<MemoryUsage> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    // free -h: Mem: total used free shared buff/cache available
    if parts.len() >= 3 {
        let total = parts[1].to_string();
        let used = parts[2].to_string();
        // Calculate percentage from raw bytes via free without -h
        // For now, parse the human-readable values
        let total_bytes = parse_human_bytes(&total);
        let used_bytes = parse_human_bytes(&used);
        let percent = if total_bytes > 0 {
            ((used_bytes as f64 / total_bytes as f64) * 100.0) as u32
        } else {
            0
        };
        Some(MemoryUsage {
            total,
            used,
            percent,
        })
    } else {
        None
    }
}

/// Parse human-readable byte strings (e.g. "7.6Gi", "2.2G", "150M") to bytes.
fn parse_human_bytes(s: &str) -> u64 {
    let s = s.trim();
    // Try to split into number and suffix
    let mut num_end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || c == '.' {
            num_end = i + c.len_utf8();
        } else {
            break;
        }
    }

    if num_end == 0 {
        return 0;
    }

    let num: f64 = match s[..num_end].parse() {
        Ok(n) => n,
        Err(_) => return 0,
    };

    let suffix = s[num_end..].trim().to_uppercase();
    let multiplier: f64 = match suffix.as_str() {
        "K" | "KI" | "KB" | "KIB" => 1024.0,
        "M" | "MI" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GI" | "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "T" | "TI" | "TB" | "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };

    (num * multiplier) as u64
}

/// Parse "service_name:status" line from systemctl output.
fn parse_service_status(line: &str) -> Option<ServiceStatus> {
    let (name, status) = line.split_once(':')?;
    let status_str = status.trim();
    Some(ServiceStatus {
        name: name.trim().to_string(),
        active: status_str == "active",
        status: status_str.to_string(),
    })
}

/// Generate warning messages based on health thresholds.
fn generate_warnings(health: &ServerHealth) -> Vec<String> {
    let mut warnings = Vec::new();

    if let Some(ref load) = health.load {
        let threshold = load.cpu_cores.max(1) as f64;
        if load.one > threshold {
            warnings.push(format!(
                "High load: {:.2} ({}x cores)",
                load.one,
                format_ratio(load.one, threshold)
            ));
        }
    }

    if let Some(ref disk) = health.disk {
        if disk.percent >= 90 {
            warnings.push(format!("Disk critically full: {}% used", disk.percent));
        } else if disk.percent >= 80 {
            warnings.push(format!("Disk usage high: {}% used", disk.percent));
        }
    }

    if let Some(ref mem) = health.memory {
        if mem.percent >= 90 {
            warnings.push(format!("Memory critically high: {}% used", mem.percent));
        } else if mem.percent >= 80 {
            warnings.push(format!("Memory usage high: {}% used", mem.percent));
        }
    }

    for svc in &health.services {
        if !svc.active {
            warnings.push(format!("Service down: {} ({})", svc.name, svc.status));
        }
    }

    warnings
}

fn format_ratio(value: f64, base: f64) -> String {
    format!("{:.1}", value / base)
}

fn status(id: &str, cached: bool, health_only: bool) -> CmdResult<FleetOutput> {
    let fl = fleet::load(id)?;
    let mut project_statuses = Vec::new();

    if cached {
        // Cached mode: read versions from local files (no SSH, no health)
        for project_id in &fl.project_ids {
            let proj = match project::load(project_id) {
                Ok(p) => p,
                Err(_) => continue,
            };

            let mut component_statuses = Vec::new();
            for component_id in &proj.component_ids {
                let comp_version = match component::load(component_id) {
                    Ok(comp) => version::get_component_version(&comp),
                    Err(_) => None,
                };

                component_statuses.push(FleetComponentStatus {
                    component_id: component_id.clone(),
                    version: comp_version,
                    version_source: Some("cached".to_string()),
                });
            }

            project_statuses.push(FleetProjectStatus {
                project_id: project_id.clone(),
                server_id: proj.server_id.clone(),
                components: component_statuses,
                health: None,
            });
        }
    } else {
        // Live mode (default): SSH into each server for versions and health
        for project_id in &fl.project_ids {
            let proj = match project::load(project_id) {
                Ok(p) => p,
                Err(_) => continue,
            };

            log_status!("fleet", "Checking '{}'...", project_id);

            // Collect health metrics via direct SSH
            let health = collect_health_for_project(&proj);

            if health_only {
                // Skip component version check
                project_statuses.push(FleetProjectStatus {
                    project_id: project_id.clone(),
                    server_id: proj.server_id.clone(),
                    components: vec![],
                    health,
                });
                continue;
            }

            // Use the deploy check infrastructure to get remote versions via SSH
            let config = DeployConfig {
                component_ids: vec![],
                all: true,
                outdated: false,
                dry_run: false,
                check: true,
                force: false,
                skip_build: true,
                keep_deps: false,
                expected_version: None,
                no_pull: true,
                head: true,
            };

            match deploy::run(project_id, &config) {
                Ok(result) => {
                    let mut component_statuses = Vec::new();
                    for comp_result in &result.results {
                        component_statuses.push(FleetComponentStatus {
                            component_id: comp_result.id.clone(),
                            version: comp_result.remote_version.clone(),
                            version_source: Some("live".to_string()),
                        });
                    }

                    project_statuses.push(FleetProjectStatus {
                        project_id: project_id.clone(),
                        server_id: proj.server_id.clone(),
                        components: component_statuses,
                        health,
                    });
                }
                Err(e) => {
                    // SSH failed for versions — fall back to cached, but keep whatever health we got
                    log_status!(
                        "fleet",
                        "Warning: could not reach '{}' — falling back to cached versions: {}",
                        project_id,
                        e
                    );

                    let mut component_statuses = Vec::new();
                    for component_id in &proj.component_ids {
                        let comp_version = match component::load(component_id) {
                            Ok(comp) => version::get_component_version(&comp),
                            Err(_) => None,
                        };

                        component_statuses.push(FleetComponentStatus {
                            component_id: component_id.clone(),
                            version: comp_version,
                            version_source: Some("cached".to_string()),
                        });
                    }

                    project_statuses.push(FleetProjectStatus {
                        project_id: project_id.clone(),
                        server_id: proj.server_id.clone(),
                        components: component_statuses,
                        health,
                    });
                }
            }
        }
    }

    Ok((
        FleetOutput {
            command: "fleet.status".to_string(),
            id: Some(id.to_string()),
            extra: FleetExtra {
                status: Some(project_statuses),
                ..Default::default()
            },
            ..Default::default()
        },
        0,
    ))
}

/// Collect health metrics for a project by SSH-ing into its server.
/// Returns None if the server can't be reached or isn't configured.
fn collect_health_for_project(proj: &Project) -> Option<ServerHealth> {
    let server_id = proj.server_id.as_deref()?;
    let srv = server::load(server_id).ok()?;
    let client = SshClient::from_server(&srv, server_id).ok()?;
    Some(collect_server_health(&client, &proj.services))
}

fn check(id: &str, only_outdated: bool) -> CmdResult<FleetOutput> {
    let fl = fleet::load(id)?;
    let mut project_checks = Vec::new();
    let mut summary = FleetCheckSummary {
        total_projects: fl.project_ids.len() as u32,
        ..Default::default()
    };

    for project_id in &fl.project_ids {
        log_status!("fleet", "Checking project '{}'...", project_id);

        // Use existing deploy check infrastructure
        let config = DeployConfig {
            component_ids: vec![],
            all: true,
            outdated: false,
            dry_run: false,
            check: true,
            force: false,
            skip_build: true,
            keep_deps: false,
            expected_version: None,
            no_pull: true, // Fleet checks are read-only
            head: true,    // Fleet checks don't build — skip tag checkout
        };

        match deploy::run(project_id, &config) {
            Ok(result) => {
                summary.projects_checked += 1;

                let proj = project::load(project_id).ok();
                let mut component_checks = Vec::new();

                for comp_result in &result.results {
                    let status_str = match &comp_result.component_status {
                        Some(deploy::ComponentStatus::UpToDate) => "up_to_date",
                        Some(deploy::ComponentStatus::NeedsUpdate) => "needs_update",
                        Some(deploy::ComponentStatus::BehindRemote) => "behind_remote",
                        Some(deploy::ComponentStatus::Unknown) | None => "unknown",
                    };

                    // Count for summary
                    match status_str {
                        "up_to_date" => summary.components_up_to_date += 1,
                        "needs_update" | "behind_remote" => summary.components_needs_update += 1,
                        _ => summary.components_unknown += 1,
                    }

                    // Skip up-to-date if only_outdated
                    if only_outdated && status_str == "up_to_date" {
                        continue;
                    }

                    component_checks.push(FleetComponentCheck {
                        component_id: comp_result.id.clone(),
                        local_version: comp_result.local_version.clone(),
                        remote_version: comp_result.remote_version.clone(),
                        status: status_str.to_string(),
                    });
                }

                // Skip project entirely if only_outdated and nothing to show
                if only_outdated && component_checks.is_empty() {
                    continue;
                }

                project_checks.push(FleetProjectCheck {
                    project_id: project_id.clone(),
                    server_id: proj.and_then(|p| p.server_id),
                    status: "checked".to_string(),
                    error: None,
                    components: component_checks,
                });
            }
            Err(e) => {
                summary.projects_failed += 1;

                if !only_outdated {
                    project_checks.push(FleetProjectCheck {
                        project_id: project_id.clone(),
                        server_id: None,
                        status: "failed".to_string(),
                        error: Some(e.to_string()),
                        components: vec![],
                    });
                }
            }
        }
    }

    let exit_code = if summary.projects_failed > 0 { 1 } else { 0 };

    Ok((
        FleetOutput {
            command: "fleet.check".to_string(),
            id: Some(id.to_string()),
            extra: FleetExtra {
                check: Some(project_checks),
                summary: Some(summary),
                ..Default::default()
            },
            ..Default::default()
        },
        exit_code,
    ))
}

fn exec(id: &str, command: Vec<String>, check: bool) -> CmdResult<FleetOutput> {
    use homeboy::shell;
    use homeboy::ssh::{resolve_context, SshResolveArgs};

    if command.is_empty() {
        return Err(
            homeboy::Error::validation_missing_argument(vec!["command".to_string()])
                .with_hint("Usage: homeboy fleet exec <fleet> -- <command>".to_string()),
        );
    }

    let command_string = if command.len() == 1 {
        command[0].clone()
    } else {
        shell::quote_args(&command)
    };

    let projects = fleet::get_projects(id)?;

    if projects.is_empty() {
        return Err(homeboy::Error::validation_invalid_argument(
            "fleet",
            "Fleet has no projects",
            Some(id.to_string()),
            None,
        ));
    }

    let mut results: Vec<FleetExecProjectResult> = Vec::new();
    let mut summary = FleetExecSummary {
        total: projects.len() as u32,
        ..Default::default()
    };

    for proj in &projects {
        let server_id = proj.server_id.clone();

        // Check mode: just show the plan
        if check {
            let effective_cmd = match &proj.base_path {
                Some(bp) => format!("cd {} && {}", shell::quote_path(bp), &command_string),
                None => command_string.clone(),
            };

            results.push(FleetExecProjectResult {
                project_id: proj.id.clone(),
                server_id: server_id.clone(),
                base_path: proj.base_path.clone(),
                command: effective_cmd,
                status: "planned".to_string(),
                ..Default::default()
            });
            continue;
        }

        homeboy::log_status!("fleet", "Executing on '{}'...", proj.id);

        // Resolve SSH context via project
        let resolve_result = match resolve_context(&SshResolveArgs {
            id: None,
            project: Some(proj.id.clone()),
            server: None,
        }) {
            Ok(r) => r,
            Err(e) => {
                summary.failed += 1;
                results.push(FleetExecProjectResult {
                    project_id: proj.id.clone(),
                    server_id: server_id.clone(),
                    base_path: proj.base_path.clone(),
                    command: command_string.clone(),
                    status: "failed".to_string(),
                    error: Some(e.to_string()),
                    ..Default::default()
                });
                continue;
            }
        };

        let client = match SshClient::from_server(&resolve_result.server, &resolve_result.server_id)
        {
            Ok(c) => c,
            Err(e) => {
                summary.failed += 1;
                results.push(FleetExecProjectResult {
                    project_id: proj.id.clone(),
                    server_id: server_id.clone(),
                    base_path: proj.base_path.clone(),
                    command: command_string.clone(),
                    status: "failed".to_string(),
                    error: Some(e.to_string()),
                    ..Default::default()
                });
                continue;
            }
        };

        // Build effective command with cd to base_path if available
        let effective_cmd = match &resolve_result.base_path {
            Some(bp) => format!("cd {} && {}", shell::quote_path(bp), &command_string),
            None => command_string.clone(),
        };

        let output = client.execute(&effective_cmd);

        if output.success {
            summary.succeeded += 1;
        } else {
            summary.failed += 1;
        }

        results.push(FleetExecProjectResult {
            project_id: proj.id.clone(),
            server_id: server_id.clone(),
            base_path: proj.base_path.clone(),
            command: effective_cmd,
            status: if output.success {
                "success".to_string()
            } else {
                "failed".to_string()
            },
            stdout: Some(output.stdout),
            stderr: Some(output.stderr),
            exit_code: Some(output.exit_code),
            error: None,
        });
    }

    if check {
        summary.skipped = summary.total;
    }

    let exit_code = if summary.failed > 0 { 1 } else { 0 };

    Ok((
        FleetOutput {
            command: "fleet.exec".to_string(),
            id: Some(id.to_string()),
            extra: FleetExtra {
                exec: Some(results),
                exec_summary: Some(summary),
                ..Default::default()
            },
            ..Default::default()
        },
        exit_code,
    ))
}

fn sync(
    _id: &str,
    _categories: Option<Vec<String>>,
    _dry_run: bool,
    _leader_override: Option<String>,
) -> CmdResult<FleetOutput> {
    Err(homeboy::Error::validation_invalid_argument(
        "fleet sync",
        "fleet sync has been deprecated. Use 'homeboy deploy' to sync files across servers. \
         Register your agent workspace as a component and deploy it like any other component.",
        None,
        None,
    )
    .with_hint("homeboy deploy <component> --fleet <fleet>".to_string())
    .with_hint("See: https://github.com/Extra-Chill/homeboy/issues/101".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uptime_p() {
        assert_eq!(
            parse_uptime("up 10 days, 3 hours, 22 minutes"),
            "10 days, 3 hours, 22 minutes"
        );
    }

    #[test]
    fn test_parse_uptime_p_short() {
        assert_eq!(parse_uptime("up 45 minutes"), "45 minutes");
    }

    #[test]
    fn test_parse_uptime_fallback() {
        // Full `uptime` output when `uptime -p` not available
        let line = " 14:32:01 up 10 days,  3:22,  1 user,  load average: 0.02, 0.08, 0.08";
        let result = parse_uptime(line);
        assert_eq!(result, "10 days,  3:22");
    }

    #[test]
    fn test_parse_load_average() {
        let load = parse_load_average("0.02 0.08 0.08 1/234 12345", None).unwrap();
        assert!((load.one - 0.02).abs() < 0.001);
        assert!((load.five - 0.08).abs() < 0.001);
        assert!((load.fifteen - 0.08).abs() < 0.001);
        assert_eq!(load.cpu_cores, 1); // default when no existing
    }

    #[test]
    fn test_parse_load_average_preserves_cores() {
        let existing = LoadAverage {
            cpu_cores: 4,
            ..Default::default()
        };
        let load = parse_load_average("1.50 0.80 0.40 2/300 999", Some(&existing)).unwrap();
        assert_eq!(load.cpu_cores, 4);
        assert!((load.one - 1.50).abs() < 0.001);
    }

    #[test]
    fn test_parse_disk_usage() {
        let disk = parse_disk_usage("/dev/sda1       150G   36G  114G  25% /").unwrap();
        assert_eq!(disk.total, "150G");
        assert_eq!(disk.used, "36G");
        assert_eq!(disk.percent, 25);
    }

    #[test]
    fn test_parse_disk_usage_high() {
        let disk = parse_disk_usage("/dev/vda1        75G   69G  2.4G  97% /").unwrap();
        assert_eq!(disk.percent, 97);
    }

    #[test]
    fn test_parse_memory_usage() {
        let mem = parse_memory_usage(
            "Mem:          7.6Gi       2.2Gi       3.1Gi       0.0Ki       2.3Gi       5.1Gi",
        )
        .unwrap();
        assert_eq!(mem.total, "7.6Gi");
        assert_eq!(mem.used, "2.2Gi");
        // 2.2 / 7.6 ≈ 28%
        assert!(mem.percent >= 28 && mem.percent <= 30);
    }

    #[test]
    fn test_parse_human_bytes() {
        assert_eq!(parse_human_bytes("7.6Gi"), 8_160_437_862); // ~7.6 GiB
        assert_eq!(parse_human_bytes("2.2Gi"), 2_362_232_012); // ~2.2 GiB
        assert_eq!(parse_human_bytes("150G"), 161_061_273_600); // ~150 GB
        assert_eq!(parse_human_bytes("512M"), 536_870_912);
        assert_eq!(parse_human_bytes("1024K"), 1_048_576);
        assert_eq!(parse_human_bytes("0"), 0);
    }

    #[test]
    fn test_parse_service_status_active() {
        let svc = parse_service_status("kimaki:active").unwrap();
        assert_eq!(svc.name, "kimaki");
        assert!(svc.active);
        assert_eq!(svc.status, "active");
    }

    #[test]
    fn test_parse_service_status_inactive() {
        let svc = parse_service_status("php8.4-fpm:inactive").unwrap();
        assert_eq!(svc.name, "php8.4-fpm");
        assert!(!svc.active);
        assert_eq!(svc.status, "inactive");
    }

    #[test]
    fn test_parse_service_status_failed() {
        let svc = parse_service_status("mysql:failed").unwrap();
        assert_eq!(svc.name, "mysql");
        assert!(!svc.active);
        assert_eq!(svc.status, "failed");
    }

    #[test]
    fn test_parse_health_output_full() {
        let output = "\
---UPTIME---
up 10 days, 3 hours
---LOAD---
0.02 0.08 0.08 1/234 12345
---CPUS---
4
---DISK---
/dev/sda1       150G   36G  114G  25% /
---MEMORY---
Mem:          7.6Gi       2.2Gi       3.1Gi       0.0Ki       2.3Gi       5.1Gi
---SERVICES---
kimaki:active
nginx:active
mysql:failed
";
        let health =
            parse_health_output(output, &["kimaki".into(), "nginx".into(), "mysql".into()]);

        assert_eq!(health.uptime.as_deref(), Some("10 days, 3 hours"));
        let load = health.load.unwrap();
        assert!((load.one - 0.02).abs() < 0.001);
        assert_eq!(load.cpu_cores, 4);
        let disk = health.disk.unwrap();
        assert_eq!(disk.percent, 25);
        let mem = health.memory.unwrap();
        assert_eq!(mem.total, "7.6Gi");
        assert_eq!(health.services.len(), 3);
        assert!(health.services[0].active);
        assert!(!health.services[2].active);
        // Should have a warning for mysql being down
        assert!(health.warnings.iter().any(|w| w.contains("mysql")));
    }

    #[test]
    fn test_parse_health_output_no_services() {
        let output = "\
---UPTIME---
up 2 hours, 15 minutes
---LOAD---
0.50 0.30 0.20 1/100 5678
---CPUS---
2
---DISK---
/dev/vda1        75G   30G   42G  42% /
---MEMORY---
Mem:          3.8Gi       1.5Gi       1.0Gi       0.0Ki       1.3Gi       2.0Gi
";
        let health = parse_health_output(output, &[]);

        assert_eq!(health.uptime.as_deref(), Some("2 hours, 15 minutes"));
        assert!(health.services.is_empty());
        assert!(health.warnings.is_empty()); // Nothing over threshold
    }

    #[test]
    fn test_warnings_high_load() {
        let health = ServerHealth {
            load: Some(LoadAverage {
                one: 5.0,
                five: 3.0,
                fifteen: 2.0,
                cpu_cores: 2,
            }),
            ..Default::default()
        };
        let warnings = generate_warnings(&health);
        assert!(warnings.iter().any(|w| w.contains("High load")));
    }

    #[test]
    fn test_warnings_high_disk() {
        let health = ServerHealth {
            disk: Some(DiskUsage {
                used: "135G".into(),
                total: "150G".into(),
                percent: 92,
            }),
            ..Default::default()
        };
        let warnings = generate_warnings(&health);
        assert!(warnings.iter().any(|w| w.contains("critically full")));
    }

    #[test]
    fn test_warnings_high_memory() {
        let health = ServerHealth {
            memory: Some(MemoryUsage {
                used: "7.0Gi".into(),
                total: "7.6Gi".into(),
                percent: 92,
            }),
            ..Default::default()
        };
        let warnings = generate_warnings(&health);
        assert!(warnings
            .iter()
            .any(|w| w.contains("Memory critically high")));
    }

    #[test]
    fn test_warnings_service_down() {
        let health = ServerHealth {
            services: vec![
                ServiceStatus {
                    name: "nginx".into(),
                    active: true,
                    status: "active".into(),
                },
                ServiceStatus {
                    name: "php-fpm".into(),
                    active: false,
                    status: "inactive".into(),
                },
            ],
            ..Default::default()
        };
        let warnings = generate_warnings(&health);
        assert!(warnings.iter().any(|w| w.contains("php-fpm")));
        assert!(!warnings.iter().any(|w| w.contains("nginx")));
    }

    #[test]
    fn test_no_warnings_when_healthy() {
        let health = ServerHealth {
            load: Some(LoadAverage {
                one: 0.5,
                five: 0.3,
                fifteen: 0.2,
                cpu_cores: 4,
            }),
            disk: Some(DiskUsage {
                used: "30G".into(),
                total: "150G".into(),
                percent: 20,
            }),
            memory: Some(MemoryUsage {
                used: "2G".into(),
                total: "8G".into(),
                percent: 25,
            }),
            services: vec![ServiceStatus {
                name: "nginx".into(),
                active: true,
                status: "active".into(),
            }],
            ..Default::default()
        };
        let warnings = generate_warnings(&health);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_shell_quote_service_safe() {
        assert_eq!(shell_quote_service("nginx"), "nginx");
        assert_eq!(shell_quote_service("php8.4-fpm"), "php8.4-fpm");
        assert_eq!(shell_quote_service("user@.service"), "user@.service");
    }

    #[test]
    fn test_shell_quote_service_unsafe() {
        let quoted = shell_quote_service("foo; rm -rf /");
        assert!(quoted.starts_with('\''));
    }
}
