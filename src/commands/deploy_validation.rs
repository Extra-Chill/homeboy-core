use crate::commands::deploy::DeployArgs;

pub fn validate_project_component_selection(args: &DeployArgs) -> homeboy::Result<()> {
    let has_positional_components = args.target_id.is_some() || !args.component_ids.is_empty();
    let has_selector_flag = args.all || args.outdated || args.check || args.json.is_some();

    if !has_positional_components && !has_selector_flag {
        return Err(homeboy::Error::validation_invalid_argument(
            "input",
            "Provide component IDs with --project, or add --all/--outdated/--check",
            None,
            Some(vec![
                "Deploy selected components: homeboy deploy --project <project> --component <id> --component <id>".to_string(),
                "Deploy all project components: homeboy deploy --project <project> --all".to_string(),
            ]),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_args() -> DeployArgs {
        DeployArgs {
            target_id: None,
            component_ids: vec![],
            project: None,
            component: None,
            json: None,
            all: false,
            outdated: false,
            dry_run: false,
            check: false,
            force: false,
            projects: None,
            fleet: None,
            shared: false,
            keep_deps: false,
            version: None,
            no_pull: false,
        }
    }

    #[test]
    fn allows_project_with_component_positionals() {
        let mut args = base_args();
        args.project = Some("example".to_string());
        args.target_id = Some("component-a".to_string());
        assert!(validate_project_component_selection(&args).is_ok());
    }

    #[test]
    fn allows_project_with_all_selector() {
        let mut args = base_args();
        args.project = Some("example".to_string());
        args.all = true;
        assert!(validate_project_component_selection(&args).is_ok());
    }

    #[test]
    fn rejects_project_without_components_or_selector() {
        let mut args = base_args();
        args.project = Some("example".to_string());
        let err = validate_project_component_selection(&args).unwrap_err();
        assert_eq!(err.code.as_str(), "validation.invalid_argument");
        assert!(err.message.contains("Provide component IDs with --project"));
    }
}
