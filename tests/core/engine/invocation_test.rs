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

// --- issue #2311: short invocation runtime path & sockaddr_un budget -------

#[test]
fn invocation_state_dir_lives_under_runtime_root_not_run_dir() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
            .expect("invocation guard");
        let env = guard.env_vars();
        let state_dir = value_for(&env, "HOMEBOY_INVOCATION_STATE_DIR");

        let runtime_root = invocation_runtime_root().expect("runtime root");
        assert!(
            Path::new(&state_dir).starts_with(&runtime_root),
            "state dir {} should live under runtime root {}",
            state_dir,
            runtime_root.display()
        );
        assert!(
            !Path::new(&state_dir).starts_with(run_dir.path()),
            "state dir {} must not be nested under run_dir {}",
            state_dir,
            run_dir.path().display()
        );

        run_dir.cleanup();
    });
}

#[test]
fn invocation_state_dir_fits_under_sockaddr_un_budget() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
            .expect("invocation guard");
        let env = guard.env_vars();

        for key in [
            "HOMEBOY_INVOCATION_STATE_DIR",
            "HOMEBOY_INVOCATION_ARTIFACT_DIR",
            "HOMEBOY_INVOCATION_TMP_DIR",
        ] {
            let dir = value_for(&env, key);
            // budget = path bytes + 1 separator + 32-byte filename headroom
            let needed = dir.len() + 1 + SOCKET_HEADROOM_BYTES;
            assert!(
                needed <= SUN_PATH_CAPACITY,
                "{key} = {dir} needs {needed} bytes, exceeds sockaddr_un capacity \
                 {SUN_PATH_CAPACITY}"
            );
            // Headroom must be at least 32 bytes for downstream socket names.
            let headroom = SUN_PATH_CAPACITY - dir.len() - 1;
            assert!(
                headroom >= SOCKET_HEADROOM_BYTES,
                "{key} = {dir} only leaves {headroom} bytes of headroom, \
                 less than required {SOCKET_HEADROOM_BYTES}"
            );
        }

        run_dir.cleanup();
    });
}

#[test]
fn invocation_dirs_are_unique_across_concurrent_invocations() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let mut paths = std::collections::HashSet::new();
        let mut guards = Vec::new();

        for _ in 0..16 {
            let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
                .expect("invocation guard");
            let dir = value_for(&guard.env_vars(), "HOMEBOY_INVOCATION_STATE_DIR");
            assert!(paths.insert(dir.clone()), "duplicate state dir: {dir}");
            guards.push(guard);
        }

        run_dir.cleanup();
    });
}

#[test]
fn invocation_id_path_component_is_short() {
    // The short id used in path components should be ~10 chars, far below
    // the 36-char UUID we used to embed.
    let id = short_invocation_id();
    assert!(id.len() <= 12, "short id should be <= 12 chars: {id}");
    assert!(id.len() >= 8, "short id should be >= 8 chars: {id}");
}

#[test]
fn invocation_runtime_root_honors_override_env() {
    // This test sets/restores HOMEBOY_INVOCATION_RUNTIME_DIR explicitly to
    // verify the env-driven fallback selection.
    let prior = std::env::var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV).ok();
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, dir.path());

    let root = invocation_runtime_root().expect("runtime root");
    assert_eq!(root, dir.path());

    match prior {
        Some(value) => std::env::set_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV, value),
        None => std::env::remove_var(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV),
    }
}

#[test]
fn enforce_path_budget_rejects_overlong_paths() {
    let mut s = String::from("/");
    while s.len() < SUN_PATH_CAPACITY {
        s.push('z');
    }
    let path = std::path::PathBuf::from(s);
    let err = enforce_path_budget(&path).expect_err("overlong path should fail");
    let message = err.to_string();
    assert!(
        message.contains("sockaddr_un"),
        "error should mention sockaddr_un: {message}"
    );
    assert!(
        message.contains(HOMEBOY_INVOCATION_RUNTIME_DIR_ENV),
        "error should suggest the override env: {message}"
    );
}

#[test]
fn invocation_drop_cleans_up_root_directory() {
    with_isolated_home(|_| {
        let run_dir = RunDir::create().expect("run dir");
        let state_dir = {
            let guard = InvocationGuard::acquire(&run_dir, &InvocationRequirements::default())
                .expect("invocation guard");
            let env = guard.env_vars();
            std::path::PathBuf::from(value_for(&env, "HOMEBOY_INVOCATION_STATE_DIR"))
        };
        assert!(
            !state_dir.exists(),
            "invocation state dir should be removed on Drop: {}",
            state_dir.display()
        );

        run_dir.cleanup();
    });
}
