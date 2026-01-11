use serde_json::Value;
use std::collections::HashMap;

use crate::module::ModuleManifest;
use crate::{Error, Result};

use super::{ComponentConfiguration, InstalledModuleConfig, ProjectConfiguration};

pub struct ModuleScope;

impl ModuleScope {
    pub fn effective_settings(
        module_id: &str,
        app: Option<&InstalledModuleConfig>,
        project: Option<&ProjectConfiguration>,
        component: Option<&ComponentConfiguration>,
    ) -> HashMap<String, Value> {
        let mut settings = HashMap::new();

        if let Some(app) = app {
            settings.extend(app.settings.clone());
        }

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

        settings
    }

    pub fn validate_project_compatibility(
        module: &ModuleManifest,
        project: &ProjectConfiguration,
    ) -> Result<()> {
        let Some(requires) = module.requires.as_ref() else {
            return Ok(());
        };

        if let Some(project_type) = requires.project_type.as_deref() {
            if project.project_type != project_type {
                return Err(Error::validation_invalid_argument(
                    "project.projectType",
                    format!(
                        "Module '{}' requires projectType '{}', but project is '{}'",
                        module.id, project_type, project.project_type
                    ),
                    None,
                    None,
                ));
            }
        }

        if let Some(required_components) = requires.components.as_ref() {
            for required in required_components {
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
        }

        Ok(())
    }

    pub fn resolve_component_scope(
        module: &ModuleManifest,
        project: &ProjectConfiguration,
        component_id: Option<&str>,
    ) -> Result<Option<String>> {
        let required_components = module
            .requires
            .as_ref()
            .and_then(|r| r.components.as_ref())
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
