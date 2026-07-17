mod common;

use std::fs;

use serde_json::{Value, json};

use common::{
    TestDir, operations_log, run_loom, run_loom_with_env, run_loom_with_env_and_cwd, write_file,
    write_skill,
};

#[test]
fn use_plan_is_read_only_before_apply() {
    let root = TestDir::new("use-plan");
    let workspace = TestDir::new("use-plan-workspace");
    write_skill(
        root.path(),
        "pdf-helper",
        "---\nname: pdf-helper\ndescription: Use when working with PDF documents in an agent workflow.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude,codex",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );

    assert!(output.status.success(), "use plan should pass: {env}");
    assert_eq!(env["cmd"], json!("use"));
    assert_eq!(env["data"]["dry_run"], json!(true));
    assert_eq!(env["data"]["steps"].as_array().map(Vec::len), Some(2));
    assert!(
        env["data"]["next_actions"][0]
            .as_str()
            .is_some_and(|command| command.contains("--apply")),
        "plan should point at explicit apply command: {env}"
    );
    assert!(
        !root.path().join("state/registry").exists(),
        "plan mode must not initialize registry state"
    );
    assert!(
        !root.path().join("targets").exists(),
        "plan mode must not create managed targets"
    );
}

#[test]
fn use_apply_projects_local_skill_without_manual_ids() {
    let root = TestDir::new("use-apply");
    let source = TestDir::new("use-apply-source");
    let workspace = TestDir::new("use-apply-workspace");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: pdf-helper\ndescription: Use when testing the human friendly use flow end to end.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");

    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (output, env) = run_loom(
        root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--scope",
            "project",
            "--workspace",
            workspace_arg,
            "--profile",
            "test",
            "--method",
            "copy",
            "--apply",
        ],
    );

    assert!(output.status.success(), "use apply should pass: {env}");
    assert_eq!(env["data"]["dry_run"], json!(false));
    let applied = &env["data"]["applied"][0];
    assert_eq!(applied["agent"], json!("claude"));
    assert!(applied["target"]["target_id"].as_str().is_some());
    assert!(applied["binding"]["binding_id"].as_str().is_some());
    assert!(applied["projection"]["instance_id"].as_str().is_some());
    assert!(applied["operation_ids"]["target"].as_str().is_some());
    assert!(applied["operation_ids"]["binding"].as_str().is_some());
    assert!(applied["operation_ids"]["projection"].as_str().is_some());
    let preflight = env["data"]["next_actions"]
        .as_array()
        .expect("next actions")
        .iter()
        .filter_map(Value::as_str)
        .find(|action| action.contains("agent preflight"))
        .expect("complete preflight command");
    assert!(preflight.contains("--agent claude"), "{preflight}");
    assert!(preflight.contains("--workspace"), "{preflight}");
    assert!(preflight.contains("--skill pdf-helper"), "{preflight}");

    let projection_path = applied["projection"]["materialized_path"]
        .as_str()
        .expect("projection path");
    assert!(
        fs::read_to_string(format!("{projection_path}/SKILL.md"))
            .expect("read projected skill")
            .contains("PDF helper"),
        "use apply should materialize the selected local skill"
    );

    let (output, env) = run_loom(root.path(), &["workspace", "binding", "list"]);
    assert!(output.status.success(), "binding list should pass: {env}");
    assert_eq!(env["data"]["count"], Value::from(1));

    let (output, env) = run_loom(root.path(), &["target", "list"]);
    assert!(output.status.success(), "target list should pass: {env}");
    assert_eq!(env["data"]["count"], Value::from(1));
}

#[test]
fn use_apply_keeps_codex_and_gemini_cli_managed_roots_distinct() {
    let root = TestDir::new("use-codex-gemini");
    let home = TestDir::new("use-codex-gemini-home");
    let workspace = TestDir::new("use-codex-gemini-workspace");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing shared agent roots.\n---\n# Demo\n",
    );
    let home_str = home.path().display().to_string();
    let workspace_str = workspace.path().display().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_str)],
        &[
            "use",
            "demo",
            "--agents",
            "codex,gemini-cli",
            "--scope",
            "project",
            "--workspace",
            &workspace_str,
            "--apply",
        ],
    );
    assert!(output.status.success(), "multi-agent use failed: {env}");
    let applied = env["data"]["applied"].as_array().expect("applied");
    assert_eq!(applied.len(), 2);
    let paths = applied
        .iter()
        .map(|row| row["target"]["path"].as_str().expect("target path"))
        .collect::<Vec<_>>();
    assert!(paths.iter().any(|path| path.ends_with(".agents/skills")));
    assert!(paths.iter().any(|path| path.ends_with(".gemini/skills")));
    assert_ne!(paths[0], paths[1]);
}

#[test]
fn gemini_user_root_uses_explicit_or_process_workspace_environment() {
    let root = TestDir::new("use-gemini-user-workspace-root");
    let home = TestDir::new("use-gemini-user-workspace-home");
    let workspace = TestDir::new("use-gemini-user-workspace");
    let redirected_home = TestDir::new("use-gemini-user-redirected-home");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Gemini workspace redirect fixture.\n---\n# Demo\n",
    );
    write_file(
        &workspace.path().join(".gemini/.env"),
        &format!("GEMINI_CLI_HOME={}\n", redirected_home.path().display()),
    );
    let home_arg = home.path().display().to_string();
    let workspace_arg = workspace.path().display().to_string();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", &home_arg), ("GEMINI_CLI_TRUST_WORKSPACE", "true")],
        &[
            "use",
            "demo",
            "--agents",
            "gemini-cli",
            "--scope",
            "user",
            "--workspace",
            &workspace_arg,
        ],
    );
    assert!(output.status.success(), "Gemini use plan failed: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        json!(redirected_home.path().join(".gemini/skills"))
    );

    let envs = [
        ("HOME", home_arg.as_str()),
        ("GEMINI_CLI_TRUST_WORKSPACE", "true"),
    ];
    let (implicit_output, implicit_env) = run_loom_with_env_and_cwd(
        root.path(),
        workspace.path(),
        &envs,
        &["use", "demo", "--agents", "gemini-cli", "--scope", "user"],
    );
    assert!(
        implicit_output.status.success(),
        "implicit-workspace Gemini use plan failed: {implicit_env}"
    );
    assert_eq!(
        implicit_env["data"]["steps"][0]["target_path"],
        json!(redirected_home.path().join(".gemini/skills"))
    );

    let (activate_output, activate_env) = run_loom_with_env_and_cwd(
        root.path(),
        workspace.path(),
        &envs,
        &["skill", "activate", "demo", "--agent", "gemini-cli"],
    );
    assert!(
        activate_output.status.success(),
        "implicit-workspace Gemini activation failed: {activate_env}"
    );
    assert_eq!(
        activate_env["data"]["target"]["path"],
        json!(redirected_home.path().join(".gemini/skills"))
    );

    let explicit_activate_root = TestDir::new("activate-gemini-explicit-workspace-root");
    let explicit_workspace = TestDir::new("activate-gemini-explicit-workspace");
    let explicit_redirected_home = TestDir::new("activate-gemini-explicit-redirected-home");
    write_skill(
        explicit_activate_root.path(),
        "demo",
        "---\nname: demo\ndescription: Gemini explicit workspace fixture.\n---\n# Demo\n",
    );
    write_file(
        &explicit_workspace.path().join(".gemini/.env"),
        &format!(
            "GEMINI_CLI_HOME={}\n",
            explicit_redirected_home.path().display()
        ),
    );
    let explicit_workspace_arg = explicit_workspace.path().display().to_string();
    let (explicit_activate_output, explicit_activate_env) = run_loom_with_env_and_cwd(
        explicit_activate_root.path(),
        explicit_activate_root.path(),
        &envs,
        &[
            "skill",
            "activate",
            "demo",
            "--agent",
            "gemini-cli",
            "--scope",
            "user",
            "--workspace",
            &explicit_workspace_arg,
        ],
    );
    assert!(
        explicit_activate_output.status.success(),
        "explicit-workspace Gemini activation failed: {explicit_activate_env}"
    );
    assert_eq!(
        explicit_activate_env["data"]["target"]["path"],
        json!(explicit_redirected_home.path().join(".gemini/skills"))
    );
}

#[test]
fn use_apply_and_skill_activate_share_project_projection_semantics() {
    let use_root = TestDir::new("use-activate-parity-use-root");
    let activate_root = TestDir::new("use-activate-parity-activate-root");
    let source = TestDir::new("use-activate-parity-source");
    let use_workspace = TestDir::new("use-activate-parity-use-workspace");
    let activate_workspace = TestDir::new("use-activate-parity-activate-workspace");
    let skill_body = "---\nname: pdf-helper\ndescription: Use when testing projection parity.\n---\n# PDF helper\n";
    write_file(&source.path().join("SKILL.md"), skill_body);
    write_skill(activate_root.path(), "pdf-helper", skill_body);

    let (output, env) = run_loom(use_root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        use_root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let use_workspace_arg = use_workspace.path().to_str().expect("use workspace");
    let (use_output, use_env) = run_loom(
        use_root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--scope",
            "project",
            "--workspace",
            use_workspace_arg,
            "--method",
            "copy",
            "--apply",
        ],
    );
    assert!(
        use_output.status.success(),
        "use apply should pass: {use_env}"
    );

    let activate_workspace_arg = activate_workspace
        .path()
        .to_str()
        .expect("activate workspace");
    let (activate_output, activate_env) = run_loom(
        activate_root.path(),
        &[
            "skill",
            "activate",
            "pdf-helper",
            "--agent",
            "claude",
            "--scope",
            "project",
            "--workspace",
            activate_workspace_arg,
            "--method",
            "copy",
        ],
    );
    assert!(
        activate_output.status.success(),
        "skill activate should pass: {activate_env}"
    );

    let use_applied = &use_env["data"]["applied"][0];
    let activate_data = &activate_env["data"];
    assert_eq!(use_applied["agent"], json!("claude"));
    assert_eq!(activate_data["target"]["agent"], json!("claude"));
    assert_eq!(use_applied["binding"]["profile_id"], json!("default"));
    assert_eq!(activate_data["binding"]["profile_id"], json!("default"));
    assert_eq!(
        use_applied["binding"]["workspace_matcher"]["kind"],
        json!("path_prefix")
    );
    assert_eq!(
        activate_data["binding"]["workspace_matcher"]["kind"],
        json!("path_prefix")
    );
    assert_eq!(use_applied["projection"]["method"], json!("copy"));
    assert_eq!(activate_data["projection"]["method"], json!("copy"));
    assert_eq!(use_applied["projection"]["health"], json!("healthy"));
    assert_eq!(activate_data["projection"]["health"], json!("healthy"));
    assert_eq!(
        use_applied["projection"]["source_tree_digest"],
        use_applied["projection"]["materialized_tree_digest"]
    );
    assert_eq!(
        activate_data["projection"]["source_tree_digest"],
        activate_data["projection"]["materialized_tree_digest"]
    );
    assert!(
        use_applied["projection"]["source_tree_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")),
        "use apply should record a source digest: {use_env}"
    );
    assert!(
        activate_data["projection"]["source_tree_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")),
        "skill activate should record a source digest: {activate_env}"
    );

    let use_projection_path = use_applied["projection"]["materialized_path"]
        .as_str()
        .expect("use projection path");
    let activate_projection_path = activate_data["projection"]["materialized_path"]
        .as_str()
        .expect("activate projection path");
    assert!(
        use_projection_path.ends_with(".claude/skills/pdf-helper"),
        "use apply target should use the claude project skill dir: {use_projection_path}"
    );
    assert!(
        activate_projection_path.ends_with(".claude/skills/pdf-helper"),
        "activation target should use the claude project skill dir: {activate_projection_path}"
    );
    assert!(
        fs::read_to_string(format!("{use_projection_path}/SKILL.md"))
            .expect("read use projection")
            .contains("PDF helper")
    );
    assert!(
        fs::read_to_string(format!("{activate_projection_path}/SKILL.md"))
            .expect("read activation projection")
            .contains("PDF helper")
    );

    assert!(operations_log(use_root.path()).contains("\"intent\":\"skill.project\""));
    assert!(operations_log(activate_root.path()).contains("\"intent\":\"skill.activate\""));
}

#[test]
fn use_user_scope_requires_adopt_before_writing_existing_agent_dirs() {
    let root = TestDir::new("use-user-adopt-required");
    let source = TestDir::new("use-user-adopt-required-source");
    let home = TestDir::new("use-user-adopt-required-home");
    let claude_dir = home.path().join(".claude/skills");
    fs::create_dir_all(&claude_dir).expect("create claude dir");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: pdf-helper\ndescription: Use when testing user-scope adoption safety.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let home_arg = home.path().to_str().expect("home path");
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", home_arg)],
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--scope",
            "user",
            "--apply",
        ],
    );

    assert!(
        !output.status.success(),
        "use without adopt should fail: {env}"
    );
    assert_eq!(env["error"]["code"], json!("TARGET_NOT_MANAGED"));
    assert_eq!(env["error"]["details"]["required_flag"], json!("--adopt"));
    assert!(
        env["error"]["next_actions"][0]["cmd"]
            .as_str()
            .is_some_and(|cmd| cmd.contains("--json") && cmd.contains("--adopt --apply")),
        "error should include the runnable adopt retry: {env}"
    );
    assert!(
        !claude_dir.join("pdf-helper").exists(),
        "unadopted use must not write into the existing agent dir"
    );
}

#[test]
fn use_user_scope_adopt_projects_to_claude_and_codex_dirs() {
    let root = TestDir::new("use-user-adopt");
    let source = TestDir::new("use-user-adopt-source");
    let home = TestDir::new("use-user-adopt-home");
    let claude_dir = home.path().join(".claude/skills");
    let codex_dir = home.path().join(".agents/skills");
    fs::create_dir_all(&claude_dir).expect("create claude dir");
    fs::create_dir_all(&codex_dir).expect("create codex dir");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: pdf-helper\ndescription: Use when testing user-scope adoption for multiple agents.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let home_arg = home.path().to_str().expect("home path");
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", home_arg)],
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude,codex",
            "--scope",
            "user",
            "--adopt",
            "--apply",
        ],
    );

    assert!(output.status.success(), "use user apply should pass: {env}");
    assert_eq!(env["data"]["dry_run"], json!(false));
    assert_eq!(env["data"]["applied"].as_array().map(Vec::len), Some(2));
    assert!(
        claude_dir.join("pdf-helper/SKILL.md").is_file(),
        "claude user skills dir should receive the skill"
    );
    assert!(
        codex_dir.join("pdf-helper/SKILL.md").is_file(),
        "codex user skills dir should receive the skill"
    );

    let applied = env["data"]["applied"].as_array().expect("applied array");
    for item in applied {
        assert_eq!(item["target"]["ownership"], json!("managed"));
        assert_eq!(item["binding"]["workspace_matcher"]["kind"], json!("name"));
        assert_eq!(item["binding"]["workspace_matcher"]["value"], json!("user"));
        assert!(item["operation_ids"]["target"].as_str().is_some());
        assert!(item["operation_ids"]["binding"].as_str().is_some());
        assert!(item["operation_ids"]["projection"].as_str().is_some());
    }
    let log = operations_log(root.path());
    assert!(log.contains("\"intent\":\"target.add\""));
    assert!(log.contains("\"intent\":\"workspace.binding.add\""));
    assert!(log.contains("\"intent\":\"skill.project\""));
}

#[test]
fn use_user_scope_adopt_upgrades_observed_target() {
    let root = TestDir::new("use-user-adopt-observed");
    let source = TestDir::new("use-user-adopt-observed-source");
    let home = TestDir::new("use-user-adopt-observed-home");
    let claude_dir = home.path().join(".claude/skills");
    fs::create_dir_all(&claude_dir).expect("create claude dir");
    write_file(
        &source.path().join("SKILL.md"),
        "---\nname: pdf-helper\ndescription: Use when testing observed target adoption.\n---\n# PDF helper\n",
    );

    let (output, env) = run_loom(root.path(), &["workspace", "init"]);
    assert!(output.status.success(), "workspace init should pass: {env}");
    let source_arg = source.path().to_str().expect("source path");
    let (output, env) = run_loom(
        root.path(),
        &["skill", "add", source_arg, "--name", "pdf-helper"],
    );
    assert!(output.status.success(), "skill add should pass: {env}");

    let claude_arg = claude_dir.to_str().expect("claude target path");
    let (output, env) = run_loom(
        root.path(),
        &[
            "target",
            "add",
            "--agent",
            "claude",
            "--path",
            claude_arg,
            "--ownership",
            "observed",
        ],
    );
    assert!(
        output.status.success(),
        "observed target add should pass: {env}"
    );

    let home_arg = home.path().to_str().expect("home path");
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("HOME", home_arg)],
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--scope",
            "user",
            "--adopt",
            "--apply",
        ],
    );

    assert!(output.status.success(), "use adopt should pass: {env}");
    let applied = &env["data"]["applied"][0];
    assert_eq!(applied["target"]["ownership"], json!("managed"));
    assert_eq!(applied["target_noop"], json!(false));
    assert!(claude_dir.join("pdf-helper/SKILL.md").is_file());
    let log = operations_log(root.path());
    assert!(log.contains("\"intent\":\"target.adopt\""));
}

#[test]
fn use_target_root_is_exact_directory() {
    let root = TestDir::new("use-target-root-exact");
    let target_root = TestDir::new("use-target-root-exact-dir");
    write_skill(
        root.path(),
        "pdf-helper",
        "---\nname: pdf-helper\ndescription: Use when testing exact target roots.\n---\n# PDF helper\n",
    );

    let target_arg = target_root.path().to_str().expect("target root path");
    let (output, env) = run_loom(
        root.path(),
        &[
            "use",
            "pdf-helper",
            "--agents",
            "claude",
            "--target-root",
            target_arg,
        ],
    );

    assert!(output.status.success(), "use plan should pass: {env}");
    assert_eq!(
        env["data"]["steps"][0]["target_path"],
        json!(target_root.path().display().to_string())
    );
    assert!(
        !env["data"]["steps"][0]["target_path"]
            .as_str()
            .expect("target path")
            .ends_with("/claude/skills"),
        "--target-root must not append <agent>/skills"
    );
}
