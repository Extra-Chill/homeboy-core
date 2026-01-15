use serde::{Deserialize, Serialize};

use std::collections::{HashMap, VecDeque};

use crate::component::{self, Component};
use crate::error::{Error, Result};
use crate::module::{self, ModuleManifest};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]

pub struct ReleaseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<ReleaseStep>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ReleaseStep {
    pub id: String,
    #[serde(rename = "type")]
    pub step_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ReleasePlan {
    pub component_id: String,
    pub enabled: bool,
    pub steps: Vec<ReleasePlanStep>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]

pub struct ReleasePlanStep {
    pub id: String,
    #[serde(rename = "type")]
    pub step_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub config: HashMap<String, serde_json::Value>,
    pub status: ReleasePlanStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]

pub enum ReleasePlanStatus {
    Ready,
    Missing,
    Disabled,
}

pub fn resolve_component_release(component: &Component) -> Option<ReleaseConfig> {
    component.release.clone()
}

pub fn plan(component_id: &str, module_id: Option<&str>) -> Result<ReleasePlan> {
    let component = component::load(component_id)?;
    let module = match module_id {
        Some(id) => {
            let suggestions = module::available_module_ids();
            Some(
                module::load_module(id)
                    .ok_or_else(|| Error::module_not_found(id.to_string(), suggestions))?,
            )
        }
        None => None,
    };
    let release = resolve_component_release(&component).ok_or_else(|| {
        Error::validation_invalid_argument(
            "release",
            "Release configuration is missing",
            Some(component_id.to_string()),
            None,
        )
        .with_hint(format!(
            "Use 'homeboy component set {} --json' to add a release block",
            component_id
        ))
        .with_hint("See 'homeboy docs commands/release' for examples")
    })?;

    let enabled = release.enabled.unwrap_or(true);
    let mut warnings = Vec::new();
    let ordered = order_steps(&release.steps, &mut warnings)?;
    let steps: Vec<ReleasePlanStep> = ordered
        .into_iter()
        .map(|step| to_plan_step(&step, module.as_ref(), enabled))
        .collect();
    let hints = build_plan_hints(component_id, &steps, module.as_ref());

    Ok(ReleasePlan {
        component_id: component_id.to_string(),
        enabled,
        steps,
        warnings,
        hints,
    })
}

fn order_steps(steps: &[ReleaseStep], warnings: &mut Vec<String>) -> Result<Vec<ReleaseStep>> {
    if steps.len() <= 1 {
        return Ok(steps.to_vec());
    }

    let mut id_index = HashMap::new();
    for (idx, step) in steps.iter().enumerate() {
        if id_index.contains_key(&step.id) {
            return Err(Error::validation_invalid_argument(
                "release.steps",
                format!("Duplicate release step id '{}'", step.id),
                None,
                None,
            ));
        }
        id_index.insert(step.id.clone(), idx);
    }

    let mut indegree = vec![0usize; steps.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); steps.len()];

    for (idx, step) in steps.iter().enumerate() {
        for need in &step.needs {
            if let Some(&parent_idx) = id_index.get(need) {
                indegree[idx] += 1;
                dependents[parent_idx].push(idx);
            } else {
                return Err(Error::validation_invalid_argument(
                    "release.steps",
                    format!("Step '{}' depends on unknown step '{}'", step.id, need),
                    None,
                    None,
                ));
            }
        }
    }

    let mut queue = VecDeque::new();
    for (idx, count) in indegree.iter().enumerate() {
        if *count == 0 {
            queue.push_back(idx);
        }
    }

    let mut ordered = Vec::with_capacity(steps.len());
    while let Some(idx) = queue.pop_front() {
        ordered.push(steps[idx].clone());
        for &child in &dependents[idx] {
            if indegree[child] > 0 {
                indegree[child] -= 1;
            }
            if indegree[child] == 0 {
                queue.push_back(child);
            }
        }
    }

    if ordered.len() != steps.len() {
        let pending: Vec<String> = steps
            .iter()
            .enumerate()
            .filter(|(idx, _)| indegree[*idx] > 0)
            .map(|(_, step)| step.id.clone())
            .collect();
        return Err(Error::validation_invalid_argument(
            "release.steps",
            "Release steps contain a cycle".to_string(),
            None,
            Some(pending),
        ));
    }

    if steps.iter().any(|step| !step.needs.is_empty()) {
        warnings.push("Release steps reordered based on dependencies".to_string());
    }

    Ok(ordered)
}

fn to_plan_step(
    step: &ReleaseStep,
    module: Option<&ModuleManifest>,
    enabled: bool,
) -> ReleasePlanStep {
    let mut missing = Vec::new();
    let status = if !enabled {
        ReleasePlanStatus::Disabled
    } else {
        let supported = is_step_supported(step, module, &mut missing);
        if supported {
            ReleasePlanStatus::Ready
        } else {
            ReleasePlanStatus::Missing
        }
    };

    ReleasePlanStep {
        id: step.id.clone(),
        step_type: step.step_type.clone(),
        label: step.label.clone(),
        needs: step.needs.clone(),
        config: step.config.clone(),
        status,
        missing,
    }
}

fn is_step_supported(
    step: &ReleaseStep,
    module: Option<&ModuleManifest>,
    missing: &mut Vec<String>,
) -> bool {
    let step_type = step.step_type.as_str();
    if is_core_step(step_type) {
        return true;
    }

    let action_id = format!("release.{}", step_type);
    if let Some(module) = module {
        if module.actions.iter().any(|action| action.id == action_id) {
            return true;
        }
    }

    missing.push(format!("Missing action '{}'", action_id));
    false
}

fn is_core_step(step_type: &str) -> bool {
    matches!(
        step_type,
        "build" | "changelog" | "version" | "git.tag" | "git.push" | "changes"
    )
}

fn build_plan_hints(
    component_id: &str,
    steps: &[ReleasePlanStep],
    module: Option<&ModuleManifest>,
) -> Vec<String> {
    let mut hints = Vec::new();
    if steps.is_empty() {
        hints.push("Release plan has no steps".to_string());
    }

    if steps
        .iter()
        .any(|step| matches!(step.status, ReleasePlanStatus::Missing))
    {
        match module {
            Some(module) => {
                hints.push(format!(
                    "Add module actions like 'release.<step_type>' in {}",
                    module.id
                ));
            }
            None => {
                hints.push("Provide --module to resolve module release actions".to_string());
            }
        }
    }

    if !hints.is_empty() {
        hints.push(format!(
            "Update release config with: homeboy component set {} --json",
            component_id
        ));
    }

    hints
}
