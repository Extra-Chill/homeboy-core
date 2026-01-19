use crate::component::Component;
use crate::error::{Error, Result};
use crate::project::Project;
use std::collections::HashMap;

use super::manifest::ModuleManifest;
use super::load_module;

/// Settings resolution for modules with project/component context.
pub struct ModuleScope;

impl ModuleScope {
    pub fn effective_settings(
        module_id: &str,
        project: Option<&Project>,
        component: Option<&Component>,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let mut settings = HashMap::new();

        if let Some(project) = project {
            if let Some(project_modules) = project.modules.as_ref() {
                if let Some(project_config) = project_modules.get(module_id) {
                    settings.extend(project_config.settings.clone());
                }
            }
        }

        if let Some(component) = component {
            if let Some(component_modules) = component.modules.as_ref() {
                if let Some(component_config) = component_modules.get(module_id) {
                    settings.extend(component_config.settings.clone());
                }
            }
        }

        Ok(settings)
    }

    pub fn validate_project_compatibility(
        module: &ModuleManifest,
        project: &Project,
    ) -> Result<()> {
        let Some(requires) = module.requires.as_ref() else {
            return Ok(());
        };

        // Required modules must be installed globally
        for required_module in &requires.modules {
            if load_module(required_module).is_err() {
                return Err(Error::validation_invalid_argument(
                    "modules",
                    format!(
                        "Module '{}' requires module '{}', but it is not installed",
                        module.id, required_module
                    ),
                    None,
                    None,
                ));
            }
        }

        // Required components must be linked to the project
        for required in &requires.components {
            if !project.component_ids.iter().any(|c| c == required) {
                return Err(Error::validation_invalid_argument(
                    "project.componentIds",
                    format!(
                        "Module '{}' requires component '{}', but project does not include it",
                        module.id, required
                    ),
                    None,
                    None,
                ));
            }
        }

        Ok(())
    }

    pub fn resolve_component_scope(
        module: &ModuleManifest,
        project: &Project,
        component_id: Option<&str>,
    ) -> Result<Option<String>> {
        let required_components = module
            .requires
            .as_ref()
            .map(|r| &r.components)
            .filter(|c| !c.is_empty());

        let Some(required_components) = required_components else {
            return Ok(component_id.map(str::to_string));
        };

        let matching_component_ids: Vec<String> = required_components
            .iter()
            .filter(|required_id| project.component_ids.iter().any(|id| id == *required_id))
            .cloned()
            .collect();

        if matching_component_ids.is_empty() {
            return Err(Error::validation_invalid_argument(
                "project.componentIds",
                format!(
                    "Module '{}' requires components {:?}; none are configured for this project",
                    module.id, required_components
                ),
                None,
                None,
            ));
        }

        if let Some(component_id) = component_id {
            if !matching_component_ids.iter().any(|c| c == component_id) {
                return Err(Error::validation_invalid_argument(
                    "component",
                    format!(
                        "Module '{}' only supports project components {:?}; --component '{}' is not compatible",
                        module.id, matching_component_ids, component_id
                    ),
                    Some(component_id.to_string()),
                    None,
                ));
            }

            return Ok(Some(component_id.to_string()));
        }

        if matching_component_ids.len() == 1 {
            return Ok(Some(matching_component_ids[0].clone()));
        }

        Err(Error::validation_invalid_argument(
            "component",
            format!(
                "Module '{}' matches multiple project components {:?}; pass --component <id>",
                module.id, matching_component_ids
            ),
            None,
            None,
        ))
    }
}
