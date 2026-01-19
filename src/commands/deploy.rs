use clap::Args;
use serde::Serialize;

use homeboy::deploy::{self, ComponentDeployResult, DeployConfig, DeploySummary};

use super::CmdResult;

#[derive(Args)]
pub struct DeployArgs {
    /// Project ID (positional, or use --project flag)
    pub project_id: Option<String>,

    /// Project ID (alternative to positional)
    #[arg(short = 'p', long)]
    pub project: Option<String>,

    /// JSON input spec for bulk operations
    #[arg(long)]
    pub json: Option<String>,

    /// Component IDs to deploy (positional)
    pub component_ids: Vec<String>,

    /// Component ID to deploy (can be repeated, alternative to positional)
    #[arg(short = 'c', long = "component")]
    pub component_flags: Vec<String>,

    /// Deploy all configured components
    #[arg(long)]
    pub all: bool,

    /// Deploy only outdated components
    #[arg(long)]
    pub outdated: bool,

    /// Preview what would be deployed without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Check component status without building or deploying
    #[arg(long)]
    pub check: bool,
}

#[derive(Serialize)]

pub struct DeployOutput {
    pub command: String,
    pub project_id: String,
    pub all: bool,
    pub outdated: bool,
    pub dry_run: bool,
    pub check: bool,
    pub results: Vec<ComponentDeployResult>,
    pub summary: DeploySummary,
}

pub fn run(mut args: DeployArgs, _global: &crate::commands::GlobalArgs) -> CmdResult<DeployOutput> {
    // Resolve project_id from positional or --project flag
    let project_id = resolve_project_id(&args)?;

    // If --project was used, first positional is actually a component ID
    if args.project.is_some() {
        if let Some(first_pos) = args.project_id.take() {
            args.component_ids.insert(0, first_pos);
        }
    }

    // Check for common subcommand mistakes (only when using positional project_id)
    if args.project.is_none() {
        let subcommand_hints = ["status", "list", "show", "help"];
        if subcommand_hints.contains(&project_id.as_str()) {
            return Err(homeboy::Error::validation_invalid_argument(
                "project_id",
                format!(
                    "'{}' looks like a subcommand, but 'deploy' doesn't have subcommands. \
                      Usage: homeboy deploy <projectId> [componentIds...] [--all]",
                    project_id
                ),
                None,
                None,
            ));
        }
    }

    // Parse JSON input if provided
    if let Some(ref spec) = args.json {
        args.component_ids = deploy::parse_bulk_component_ids(spec)?;
    }

    // Merge positional and flag component IDs
    let mut all_component_ids = args.component_ids.clone();
    all_component_ids.extend(args.component_flags.iter().cloned());

    // Build config and call core orchestration
    let config = DeployConfig {
        component_ids: all_component_ids,
        all: args.all,
        outdated: args.outdated,
        dry_run: args.dry_run,
        check: args.check,
    };

    let result = deploy::run(&project_id, &config).map_err(|e| {
        if e.message.contains("No components configured for project") {
            e.with_hint(format!(
                "Run 'homeboy project components add {} <component-id>' to add components",
                project_id
            ))
            .with_hint("Run 'homeboy init' to see project context and available components")
        } else {
            e
        }
    })?;

    let exit_code = if result.summary.failed > 0 { 1 } else { 0 };

    Ok((
        DeployOutput {
            command: "deploy.run".to_string(),
            project_id,
            all: args.all,
            outdated: args.outdated,
            dry_run: args.dry_run,
            check: args.check,
            results: result.results,
            summary: result.summary,
        },
        exit_code,
    ))
}

fn resolve_project_id(args: &DeployArgs) -> Result<String, homeboy::Error> {
    // If --project flag provided, use it
    if let Some(ref project_flag) = args.project {
        let available_projects = homeboy::project::list_ids().unwrap_or_default();
        if !available_projects.contains(project_flag) {
            return Err(homeboy::Error::validation_invalid_argument(
                "project",
                format!("Project '{}' not found", project_flag),
                None,
                Some(vec![format!(
                    "Available projects: {}",
                    available_projects.join(", ")
                )]),
            ));
        }
        return Ok(project_flag.clone());
    }

    // Otherwise, first positional must be project_id
    match &args.project_id {
        Some(id) => {
            // Check if user provided component ID where project ID expected (reversed argument order)
            let available_components = homeboy::component::list_ids().unwrap_or_default();
            if available_components.contains(id) {
                return Err(homeboy::Error::validation_invalid_argument(
                    "project_id",
                    format!(
                        "'{}' is a component, not a project. \
                          Did you mean: homeboy deploy <project> {}",
                        id, id
                    ),
                    None,
                    Some(vec![
                        "Argument order: homeboy deploy <project_id> [component_ids...]".to_string(),
                        format!("Alternative: homeboy deploy {} --project <project_id>", id),
                    ]),
                ));
            }
            Ok(id.clone())
        }
        None => Err(homeboy::Error::validation_missing_argument(vec![
            "project_id".to_string(),
        ])
        .with_hint("homeboy deploy <project_id> [component_ids]")
        .with_hint("homeboy deploy <component_id> --project <project_id>")),
    }
}
