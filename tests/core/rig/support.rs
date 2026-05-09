pub(crate) fn minimal_stack(id: &str, component: &str) -> String {
    format!(
        r#"{{
            "id": "{}",
            "description": "{} stack",
            "component": "{}",
            "component_path": "${{env.DEV_ROOT}}/{}",
            "base": {{ "remote": "origin", "branch": "main" }},
            "target": {{ "remote": "origin", "branch": "dev/combined-fixes" }},
            "prs": []
        }}"#,
        id, id, component, component
    )
}
