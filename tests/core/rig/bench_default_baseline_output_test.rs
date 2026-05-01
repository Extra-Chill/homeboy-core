use super::*;

fn write_rig_with_default_baseline(
    home: &TempDir,
    rig_id: &str,
    component_id: &str,
    path: &std::path::Path,
    default_baseline_rig: &str,
) {
    write_rig(home, rig_id, component_id, path);
    let rig_path = home
        .path()
        .join(".config")
        .join("homeboy")
        .join("rigs")
        .join(format!("{}.json", rig_id));
    let mut rig_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&rig_path).expect("read rig")).expect("parse rig");
    rig_json["bench"]["default_baseline_rig"] = serde_json::json!(default_baseline_rig);
    fs::write(
        &rig_path,
        serde_json::to_string(&rig_json).expect("serialize rig"),
    )
    .expect("write rig");
}

#[test]
fn default_baseline_expansion_records_metadata_on_comparison_output() {
    with_isolated_home(|home| {
        write_bench_extension(home);
        let baseline_component = tempfile::TempDir::new().expect("baseline component");
        let candidate_component = tempfile::TempDir::new().expect("candidate component");
        write_rig(
            home,
            "studio-agent-sdk",
            "studio",
            baseline_component.path(),
        );
        write_rig_with_default_baseline(
            home,
            "studio-bfb",
            "studio",
            candidate_component.path(),
            "studio-agent-sdk",
        );

        let (output, exit_code) = run(
            run_args(
                None,
                vec!["studio-bfb".to_string()],
                vec!["rig-slow".to_string()],
            ),
            &GlobalArgs {},
        )
        .expect("default baseline expansion should run as comparison");

        assert_eq!(exit_code, 0);
        match output {
            BenchOutput::Comparison(result) => {
                let expansion = result
                    .default_baseline_expansion
                    .expect("expansion metadata");
                assert_eq!(expansion.baseline_rig, "studio-agent-sdk");
                assert_eq!(expansion.candidate_rig, "studio-bfb");
                assert_eq!(
                    expansion.execution_order,
                    vec!["studio-agent-sdk".to_string(), "studio-bfb".to_string()]
                );
                assert_eq!(expansion.opt_out_flag, "--ignore-default-baseline");
            }
            _ => panic!("expected comparison output"),
        }
    });
}

#[test]
fn default_baseline_expansion_metadata_survives_json_summary() {
    with_isolated_home(|home| {
        write_bench_extension(home);
        let baseline_component = tempfile::TempDir::new().expect("baseline component");
        let candidate_component = tempfile::TempDir::new().expect("candidate component");
        write_rig(
            home,
            "studio-agent-sdk",
            "studio",
            baseline_component.path(),
        );
        write_rig_with_default_baseline(
            home,
            "studio-bfb",
            "studio",
            candidate_component.path(),
            "studio-agent-sdk",
        );

        let mut args = run_args(
            None,
            vec!["studio-bfb".to_string()],
            vec!["rig-slow".to_string()],
        );
        args.run.json_summary = true;
        let (output, exit_code) =
            run(args, &GlobalArgs {}).expect("default baseline summary should run");

        assert_eq!(exit_code, 0);
        match output {
            BenchOutput::ComparisonSummary(result) => {
                let value = serde_json::to_value(result).expect("serialize summary");
                assert_eq!(
                    value["default_baseline_expansion"]["baseline_rig"],
                    "studio-agent-sdk"
                );
                assert_eq!(
                    value["default_baseline_expansion"]["candidate_rig"],
                    "studio-bfb"
                );
                assert_eq!(
                    value["default_baseline_expansion"]["execution_order"],
                    serde_json::json!(["studio-agent-sdk", "studio-bfb"])
                );
                assert_eq!(
                    value["default_baseline_expansion"]["opt_out_flag"],
                    "--ignore-default-baseline"
                );
            }
            _ => panic!("expected comparison summary output"),
        }
    });
}

#[test]
fn default_baseline_expansion_notice_names_order_and_opt_out() {
    let metadata = BenchDefaultBaselineExpansion {
        baseline_rig: "studio-agent-sdk".to_string(),
        candidate_rig: "studio-bfb".to_string(),
        execution_order: vec!["studio-agent-sdk".to_string(), "studio-bfb".to_string()],
        opt_out_flag: "--ignore-default-baseline",
    };

    let notice = default_baseline_notice(&metadata);

    assert!(notice.contains("studio-agent-sdk"), "got: {notice}");
    assert!(notice.contains("studio-bfb"), "got: {notice}");
    assert!(
        notice.contains("studio-agent-sdk -> studio-bfb"),
        "got: {notice}"
    );
    assert!(
        notice.contains("--ignore-default-baseline"),
        "got: {notice}"
    );
}
