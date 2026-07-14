use super::*;

#[test]
fn skill_trash_add_rejects_partial_registry_without_repairing_it() {
    let root = TestDir::new("skill-trash-partial-registry");
    let (init_output, init_env) = run_loom(root.path(), &["workspace", "init"]);
    assert_success(&init_output, &format!("workspace init: {init_env}"));
    write_activatable_skill(root.path(), "demo");
    fs::remove_dir_all(root.path().join("state/registry")).expect("remove initialized registry");
    let rules_path = root.path().join("state/registry/rules.json");
    write_file(&rules_path, "{\"schema_version\":1,\"rules\":[]}\n");
    let rules_before = fs::read(&rules_path).expect("read partial rules");

    let (output, env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);

    assert!(
        !output.status.success(),
        "partial registry unexpectedly accepted"
    );
    assert_eq!(env["error"]["code"], Value::String("STATE_CORRUPT".into()));
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert_eq!(
        fs::read(&rules_path).expect("read partial rules after"),
        rules_before
    );
    assert!(!root.path().join("state/registry/schema.json").exists());
    assert!(!root.path().join("state/registry/targets.json").exists());
}

#[test]
fn skill_trash_add_handles_missing_managed_projection() {
    let root = TestDir::new("skill-trash-missing-projection");
    let home = TestDir::new("skill-trash-missing-projection-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    fs::remove_file(&live_path).expect("remove managed projection");

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);

    assert_success(&trash_output, &format!("trash add: {trash_env}"));
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["action"],
        "missing"
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["reason"],
        "path_missing"
    );
    assert_eq!(
        read_json(&root.path().join("state/registry/projections.json"))["projections"],
        Value::Array(Vec::new())
    );
}

#[test]
fn skill_trash_add_retains_wrong_target_symlink() {
    let root = TestDir::new("skill-trash-wrong-target");
    let home = TestDir::new("skill-trash-wrong-target-home");
    let wrong = TestDir::new("skill-trash-wrong-target-payload");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    fs::remove_file(&live_path).expect("remove managed projection");
    create_dir_symlink(wrong.path(), &live_path);

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);

    assert_success(&trash_output, &format!("trash add: {trash_env}"));
    assert!(live_path.is_symlink());
    assert_eq!(
        fs::read_link(&live_path).expect("read retained link"),
        wrong.path()
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["action"],
        "retain"
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["reason"],
        "symlink_target_mismatch"
    );
}

#[test]
fn skill_trash_add_retains_copy_and_materialize_projections() {
    for method in ["copy", "materialize"] {
        let root = TestDir::new(&format!("skill-trash-{method}-projection"));
        let home = TestDir::new(&format!("skill-trash-{method}-projection-home"));
        write_activatable_skill(root.path(), "demo");
        let (activate_output, activate_env) = run_with_home(
            root.path(),
            home.path(),
            &[
                "skill", "activate", "demo", "--agent", "codex", "--method", "copy",
            ],
        );
        assert_success(&activate_output, &format!("skill activate: {activate_env}"));
        if method == "materialize" {
            for file in ["rules.json", "projections.json"] {
                let path = root.path().join("state/registry").join(file);
                let mut value = read_json(&path);
                let key = if file == "rules.json" {
                    "rules"
                } else {
                    "projections"
                };
                value[key][0]["method"] = Value::String("materialize".into());
                write_json(&path, &value);
            }
        }
        let live_path = home.path().join(".agents/skills/demo");
        assert!(live_path.is_dir() && !live_path.is_symlink());

        let (trash_output, trash_env) =
            run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);

        assert_success(&trash_output, &format!("trash {method}: {trash_env}"));
        assert!(live_path.join("SKILL.md").is_file());
        assert_eq!(
            trash_env["data"]["activation_impact"]["links"][0]["action"],
            "retain"
        );
        assert_eq!(
            trash_env["data"]["activation_impact"]["links"][0]["reason"],
            "non_symlink_projection"
        );
    }
}

#[test]
fn skill_trash_add_deduplicates_normalized_projection_paths() {
    let root = TestDir::new("skill-trash-normalized-duplicate");
    let home = TestDir::new("skill-trash-normalized-duplicate-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let projections_path = root.path().join("state/registry/projections.json");
    let mut projections = read_json(&projections_path);
    let original = projections["projections"][0].clone();
    let materialized = PathBuf::from(
        original["materialized_path"]
            .as_str()
            .expect("materialized path"),
    );
    let mut duplicate = original;
    duplicate["instance_id"] = Value::String("normalized-duplicate".into());
    duplicate["materialized_path"] = Value::String(format!(
        "{}/./{}",
        materialized.parent().expect("projection parent").display(),
        materialized
            .file_name()
            .expect("projection leaf")
            .to_string_lossy()
    ));
    projections["projections"]
        .as_array_mut()
        .expect("projection array")
        .push(duplicate);
    write_json(&projections_path, &projections);

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);

    assert_success(&trash_output, &format!("trash add: {trash_env}"));
    assert_eq!(
        trash_env["data"]["activation_impact"]["removed_projection_ids"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(!home.path().join(".agents/skills/demo").exists());
}

#[test]
fn skill_trash_add_rollback_restores_relative_symlink_exactly() {
    let root = TestDir::new("skill-trash-relative-link-rollback");
    let home = TestDir::new("skill-trash-relative-link-rollback-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    fs::remove_file(&live_path).expect("remove absolute projection");
    let relative_target = relative_path(
        live_path.parent().expect("live parent"),
        &root.path().join("skills/demo"),
    );
    create_dir_symlink(&relative_target, &live_path);
    assert_eq!(
        fs::canonicalize(&live_path).expect("resolve relative projection"),
        fs::canonicalize(root.path().join("skills/demo")).expect("resolve skill")
    );
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add must fail: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert_eq!(
        fs::read_link(&live_path).expect("read restored relative projection"),
        relative_target
    );
}

#[test]
fn skill_trash_add_reports_projection_restore_rollback_errors() {
    let root = TestDir::new("skill-trash-projection-rollback-errors");
    let home = TestDir::new("skill-trash-projection-rollback-errors-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
            ("LOOM_ROLLBACK_FAULT_INJECT", "restore_projection_path"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(!output.status.success(), "faulted trash add must fail");
    assert!(
        rollback_error_steps(&env).contains(&"restore_projection_path".to_string()),
        "missing projection rollback error details: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert!(!home.path().join(".agents/skills/demo").exists());
}

#[test]
fn skill_trash_add_failure_restores_exact_preexisting_registry_index() {
    let root = TestDir::new("skill-trash-exact-index-rollback");
    let home = TestDir::new("skill-trash-exact-index-rollback-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let targets_path = root.path().join("state/registry/targets.json");
    let mut targets = fs::read_to_string(&targets_path).expect("read targets");
    targets.push('\n');
    write_file(&targets_path, &targets);
    git_success(root.path(), &["add", "state/registry/targets.json"]);
    let cached_before = git_success(
        root.path(),
        &[
            "diff",
            "--cached",
            "--binary",
            "--",
            "state/registry/targets.json",
        ],
    );
    let status_before = git_success(root.path(), &["status", "--porcelain=v2"]);
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add must fail: {env}"
    );
    assert_eq!(
        git_success(
            root.path(),
            &[
                "diff",
                "--cached",
                "--binary",
                "--",
                "state/registry/targets.json"
            ],
        ),
        cached_before
    );
    assert_eq!(
        git_success(root.path(), &["status", "--porcelain=v2"]),
        status_before
    );
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert!(home.path().join(".agents/skills/demo").is_symlink());
}

#[test]
fn skill_trash_add_retains_directory_replacement() {
    let root = TestDir::new("skill-trash-directory-replacement");
    let home = TestDir::new("skill-trash-directory-replacement-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let live_path = home.path().join(".agents/skills/demo");
    fs::remove_file(&live_path).expect("remove managed projection");
    write_file(&live_path.join("KEEP.txt"), "user-owned directory\n");

    let (trash_output, trash_env) =
        run_with_home(root.path(), home.path(), &["skill", "trash", "add", "demo"]);

    assert_success(&trash_output, &format!("trash add: {trash_env}"));
    assert_eq!(
        fs::read_to_string(live_path.join("KEEP.txt")).expect("read retained directory file"),
        "user-owned directory\n"
    );
    assert_eq!(
        trash_env["data"]["activation_impact"]["links"][0]["reason"],
        "not_symlink"
    );
}

#[test]
fn skill_trash_add_rollback_preserves_registry_symlink_entries() {
    let root = TestDir::new("skill-trash-registry-symlink-rollback");
    let home = TestDir::new("skill-trash-registry-symlink-rollback-home");
    let external = TestDir::new("skill-trash-registry-symlink-external");
    write_file(&external.path().join("external.jsonl"), "external\n");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    let linked_entry = root
        .path()
        .join("state/registry/observations/external-link");
    create_dir_symlink(external.path(), &linked_entry);
    let original_target = fs::read_link(&linked_entry).expect("read original registry symlink");
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add must fail: {env}"
    );
    assert!(linked_entry.is_symlink());
    assert_eq!(
        fs::read_link(&linked_entry).expect("read restored registry symlink"),
        original_target
    );
    assert_eq!(
        fs::read_to_string(external.path().join("external.jsonl")).expect("read external payload"),
        "external\n"
    );
}

#[test]
fn skill_trash_add_rollback_removes_new_registry_layout() {
    let root = TestDir::new("skill-trash-absent-registry-rollback");
    let (init_output, init_env) = run_loom(root.path(), &["workspace", "init"]);
    assert_success(&init_output, &format!("workspace init: {init_env}"));
    fs::remove_dir_all(root.path().join("state/registry")).expect("remove initialized registry");
    write_activatable_skill(root.path(), "demo");

    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint")],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add must fail: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert!(!root.path().join("state/registry").exists());
}

#[test]
fn skill_trash_add_rollback_restores_legacy_registry_layout() {
    let root = TestDir::new("skill-trash-legacy-registry-rollback");
    let home = TestDir::new("skill-trash-legacy-registry-rollback-home");
    write_activatable_skill(root.path(), "demo");
    let (activate_output, activate_env) = run_with_home(
        root.path(),
        home.path(),
        &["skill", "activate", "demo", "--agent", "codex"],
    );
    assert_success(&activate_output, &format!("skill activate: {activate_env}"));
    fs::rename(
        root.path().join("state/registry"),
        root.path().join("state/v3"),
    )
    .expect("move registry to legacy layout");
    let home_value = home.path().to_string_lossy().to_string();

    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("HOME", &home_value),
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
        ],
        &["skill", "trash", "add", "demo"],
    );

    assert!(
        !output.status.success(),
        "faulted trash add must fail: {env}"
    );
    assert!(root.path().join("skills/demo/SKILL.md").is_file());
    assert!(!root.path().join("state/registry").exists());
    assert!(root.path().join("state/v3/schema.json").is_file());
    assert_eq!(
        read_json(&root.path().join("state/v3/projections.json"))["projections"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(home.path().join(".agents/skills/demo").is_symlink());
}
