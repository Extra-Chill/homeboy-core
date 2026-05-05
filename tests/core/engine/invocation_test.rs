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

#[test]
fn test_register_child_process() {
    with_isolated_home(|_| {
        let guard =
            register_child_process("inv-test", std::process::id(), None, "self".to_string())
                .expect("child record");
        assert!(guard.path.exists());

        let path = guard.path.clone();
        drop(guard);

        assert!(
            !path.exists(),
            "child record should be removed on normal exit"
        );
    });
}

#[cfg(unix)]
#[test]
fn test_cleanup_stale_child_records() {
    with_isolated_home(|_| {
        let mut child = spawn_isolated_sleep();
        let pid = child.id();
        write_test_child_record("inv-stale", 999_999, pid, Some(pid as i32));

        let cleaned = cleanup_stale_child_records().expect("cleanup stale records");

        assert_eq!(cleaned, 1);
        assert_child_exits(&mut child);
        assert!(!InvocationChildRecord::record_path("inv-stale", pid)
            .unwrap()
            .exists());
    });
}

#[cfg(unix)]
#[test]
fn test_cleanup_invocation_children() {
    with_isolated_home(|_| {
        let mut first = spawn_isolated_sleep();
        let mut second = spawn_isolated_sleep();
        let first_pid = first.id();
        let second_pid = second.id();
        write_test_child_record("inv-first", 999_999, first_pid, Some(first_pid as i32));
        write_test_child_record("inv-second", 999_999, second_pid, Some(second_pid as i32));

        let cleaned = cleanup_invocation_children("inv-first").expect("cleanup first invocation");

        assert_eq!(cleaned, 1);
        assert_child_exits(&mut first);
        assert!(pid_is_alive(second_pid as libc::pid_t));

        let _ = cleanup_invocation_children("inv-second");
        assert_child_exits(&mut second);
    });
}

fn value_for(env: &[(String, String)], key: &str) -> String {
    value_for_optional(env, key).unwrap_or_else(|| panic!("missing {key}"))
}

fn value_for_optional(env: &[(String, String)], key: &str) -> Option<String> {
    env.iter()
        .find_map(|(candidate, value)| (candidate == key).then(|| value.clone()))
}

#[cfg(unix)]
fn spawn_isolated_sleep() -> std::process::Child {
    use std::os::unix::process::CommandExt;

    let mut command = std::process::Command::new("sh");
    command.args(["-c", "sleep 30"]);
    unsafe {
        command.pre_exec(|| {
            if libc::setpgid(0, 0) == 0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        });
    }
    command.spawn().expect("spawn isolated sleep")
}

#[cfg(unix)]
fn write_test_child_record(invocation_id: &str, owner_pid: u32, root_pid: u32, pgid: Option<i32>) {
    let dir = InvocationChildRecord::children_dir(invocation_id).expect("child dir");
    std::fs::create_dir_all(&dir).expect("create child dir");
    let record = InvocationChildRecord {
        invocation_id: invocation_id.to_string(),
        owner_pid,
        owner_started_at: None,
        child: crate::engine::resource::ChildProcessIdentity {
            root_pid,
            command_label: "sleep".to_string(),
        },
        root_started_at: InvocationChildRecord::process_started_at(root_pid),
        pgid,
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    let json = serde_json::to_string_pretty(&record).expect("serialize child record");
    std::fs::write(
        InvocationChildRecord::record_path(invocation_id, root_pid).expect("record path"),
        json,
    )
    .expect("write child record");
}

#[cfg(unix)]
fn assert_child_exits(child: &mut std::process::Child) {
    for _ in 0..30 {
        if child.try_wait().expect("try wait").is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    panic!("child {} should have exited", child.id());
}

#[cfg(unix)]
fn pid_is_alive(pid: libc::pid_t) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}
