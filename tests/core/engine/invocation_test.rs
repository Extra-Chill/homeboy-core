use super::*;
use crate::engine::run_dir::RunDir;
use crate::test_support::with_isolated_home;

#[test]
fn test_env_vars() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
            .expect("invocation guard");
        let env = guard.env_vars();

        let id = value_for(&env, "HOMEBOY_INVOCATION_ID");
        assert!(id.starts_with("inv-"));
        assert!(Path::new(&value_for(&env, "HOMEBOY_INVOCATION_STATE_DIR")).is_dir());
        assert!(Path::new(&value_for(&env, "HOMEBOY_INVOCATION_ARTIFACT_DIR")).is_dir());
        assert!(Path::new(&value_for(&env, "HOMEBOY_INVOCATION_TMP_DIR")).is_dir());
        assert!(value_for_optional(&env, "HOMEBOY_INVOCATION_PORT_BASE").is_none());

        run_dir.cleanup();
    });
}

#[test]
fn port_ranges_do_not_overlap_while_leased() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let requirements = InvocationRequirements {
            port_range_size: Some(4),
            named_leases: Vec::new(),
        };

        let first = InvocationGuard::acquire(&run_dir, &requirements).expect("first lease");
        let second = InvocationGuard::acquire(&run_dir, &requirements).expect("second lease");
        let first_base: u16 = value_for(&first.env_vars(), "HOMEBOY_INVOCATION_PORT_BASE")
            .parse()
            .expect("first base");
        let first_max: u16 = value_for(&first.env_vars(), "HOMEBOY_INVOCATION_PORT_MAX")
            .parse()
            .expect("first max");
        let second_base: u16 = value_for(&second.env_vars(), "HOMEBOY_INVOCATION_PORT_BASE")
            .parse()
            .expect("second base");

        assert!(second_base > first_max);
        assert_eq!(first_max - first_base + 1, 4);

        run_dir.cleanup();
    });
}

#[test]
fn named_lease_conflicts_report_holder() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let requirements = InvocationRequirements {
            port_range_size: None,
            named_leases: vec!["playground-browser-profile".to_string()],
        };

        let _first = InvocationGuard::acquire(&run_dir, &requirements).expect("first lease");
        let err = InvocationGuard::acquire(&run_dir, &requirements).expect_err("lease conflict");
        let message = err.to_string();

        assert!(message.contains("playground-browser-profile"));
        assert!(message.contains("already held"));

        run_dir.cleanup();
    });
}

fn value_for(env: &[(String, String)], key: &str) -> String {
    value_for_optional(env, key).unwrap_or_else(|| panic!("missing {key}"))
}

fn value_for_optional(env: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .find_map(|(candidate, value)| (candidate == key).then(|| value.clone()))
}
