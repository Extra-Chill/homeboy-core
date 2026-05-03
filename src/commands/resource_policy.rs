use crate::cli_surface::Commands;

use super::doctor::resources::{DoctorOutput, ResourceRecommendation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotCommand {
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcePolicyWarning {
    pub command: &'static str,
    pub recommendation: ResourceRecommendation,
    pub message: String,
}

pub fn hot_command(command: &Commands) -> Option<HotCommand> {
    match command {
        Commands::Bench(args) if args.is_run_command() => Some(HotCommand { label: "bench" }),
        Commands::Rig(args) if args.is_hot_resource_command() => {
            Some(HotCommand { label: "rig up" })
        }
        Commands::Fleet(args) if args.is_hot_resource_command() => Some(HotCommand {
            label: "fleet exec",
        }),
        Commands::Audit(args) if args.changed_since.is_none() && !args.conventions => {
            Some(HotCommand { label: "audit" })
        }
        Commands::Lint(args) if args.is_full_workspace_run() => Some(HotCommand { label: "lint" }),
        Commands::Test(args) if args.changed_since.is_none() => Some(HotCommand { label: "test" }),
        _ => None,
    }
}

pub fn evaluate(command: HotCommand, resources: &DoctorOutput) -> Option<ResourcePolicyWarning> {
    match resources.recommendation {
        ResourceRecommendation::Ok => None,
        recommendation => Some(ResourcePolicyWarning {
            command: command.label,
            recommendation,
            message: warning_message(command.label, recommendation, resources),
        }),
    }
}

pub fn warning_for_command(
    command: &Commands,
    resources: &DoctorOutput,
) -> Option<ResourcePolicyWarning> {
    evaluate(hot_command(command)?, resources)
}

fn warning_message(
    command: &'static str,
    recommendation: ResourceRecommendation,
    resources: &DoctorOutput,
) -> String {
    let severity = match recommendation {
        ResourceRecommendation::Ok => "ok",
        ResourceRecommendation::Warm => "warm",
        ResourceRecommendation::Hot => "hot",
    };
    let reason = primary_reason(resources);
    format!(
        "Resource policy warning: machine is {severity}; starting `{command}` may skew results or add pressure. {reason} Use --force-hot to run without this warning."
    )
}

fn primary_reason(resources: &DoctorOutput) -> String {
    if resources.load.recommendation == ResourceRecommendation::Hot
        || resources.load.recommendation == ResourceRecommendation::Warm
    {
        if let Some(one) = resources.load.one {
            return format!(
                "Load average is {one:.1} across {} CPU(s).",
                resources.load.cpu_count
            );
        }
        return "Load average is elevated.".to_string();
    }

    if let Some(memory) = &resources.memory {
        if memory.recommendation == ResourceRecommendation::Hot
            || memory.recommendation == ResourceRecommendation::Warm
        {
            return format!(
                "Memory is {:.1}% used ({} MB available).",
                memory.used_percent, memory.available_mb
            );
        }
    }

    if resources.processes.recommendation == ResourceRecommendation::Hot
        || resources.processes.recommendation == ResourceRecommendation::Warm
    {
        return format!(
            "{} relevant Homeboy-adjacent process(es) are already active.",
            resources.processes.relevant_count
        );
    }

    if resources.rig_leases.recommendation == ResourceRecommendation::Hot
        || resources.rig_leases.recommendation == ResourceRecommendation::Warm
    {
        return format!(
            "{} rig run lease(s) are already active.",
            resources.rig_leases.active_count
        );
    }

    "Run `homeboy doctor resources` for details.".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::doctor::resources::{LoadSummary, ProcessSummary, RigLeaseSummary};

    fn resources(recommendation: ResourceRecommendation) -> DoctorOutput {
        DoctorOutput {
            command: "doctor.resources",
            recommendation,
            load: LoadSummary {
                one: Some(9.0),
                five: Some(7.0),
                fifteen: Some(5.0),
                cpu_count: 4,
                recommendation,
            },
            memory: None,
            processes: ProcessSummary {
                relevant_count: 0,
                top_cpu: Vec::new(),
                top_rss: Vec::new(),
                recommendation: ResourceRecommendation::Ok,
            },
            rig_leases: RigLeaseSummary {
                active_count: 0,
                leases: Vec::new(),
                recommendation: ResourceRecommendation::Ok,
            },
            notes: Vec::new(),
        }
    }

    #[test]
    fn warns_when_hot_command_runs_on_warm_or_hot_machine() {
        let warning = evaluate(
            HotCommand { label: "bench" },
            &resources(ResourceRecommendation::Hot),
        )
        .expect("hot machines warn");
        assert_eq!(warning.command, "bench");
        assert_eq!(warning.recommendation, ResourceRecommendation::Hot);
        assert!(warning.message.contains("--force-hot"));
        assert!(warning.message.contains("Load average is 9.0"));

        assert!(evaluate(
            HotCommand { label: "bench" },
            &resources(ResourceRecommendation::Warm)
        )
        .is_some());
    }

    #[test]
    fn does_not_warn_when_machine_is_ok() {
        assert!(evaluate(
            HotCommand { label: "bench" },
            &resources(ResourceRecommendation::Ok)
        )
        .is_none());
    }
}
