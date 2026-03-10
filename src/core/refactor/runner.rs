use crate::component::Component;
use crate::extension;

pub fn resolve_lint_script(component: &Component) -> crate::Result<String> {
    extension::resolve_lint_script(component)
}

pub fn resolve_test_script(component: &Component) -> crate::Result<String> {
    extension::resolve_test_script(component)
}
