mod common;

use serde_json::Value;

use common::{TestDir, run_loom, run_loom_in_cwd, write_file, write_skill};

fn assert_ok(output: &std::process::Output, env: &Value) {
    assert!(
        output.status.success(),
        "loom command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(env["ok"], Value::Bool(true));
}

fn surfaces(env: &Value) -> &[Value] {
    env["data"]["surfaces"].as_array().expect("surfaces array")
}

fn surface_with_kind<'a>(env: &'a Value, kind: &str, path_suffix: &str) -> &'a Value {
    surfaces(env)
        .iter()
        .find(|surface| {
            surface["kind"].as_str() == Some(kind)
                && surface["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(path_suffix))
        })
        .expect("matching surface")
}

fn surface_with_kind_agent<'a>(
    env: &'a Value,
    kind: &str,
    agent: &str,
    path_suffix: &str,
) -> &'a Value {
    surfaces(env)
        .iter()
        .find(|surface| {
            surface["kind"].as_str() == Some(kind)
                && surface["agent"].as_str() == Some(agent)
                && surface["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(path_suffix))
        })
        .expect("matching agent surface")
}

#[test]
fn instruction_scan_discovers_agents_nested_and_cursor_without_registry_writes() {
    let root = TestDir::new("instruction-scan-root");
    let workspace = TestDir::new("instruction-scan-workspace");
    write_file(
        &workspace.path().join("AGENTS.md"),
        "# Agents\n\nWorkflow steps: run cargo test and lint in CI.\n",
    );
    write_file(
        &workspace.path().join("crates/app/AGENTS.md"),
        "# App\n\nUse when nested package rules apply.\n",
    );
    write_file(
        &workspace.path().join(".cursor/rules/security.mdc"),
        "# Cursor\n\nNever skip security review steps.\n",
    );
    write_file(
        &workspace
            .path()
            .join(".agents/skills/not-instruction/AGENTS.md"),
        "# Not an instruction surface\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "instruction",
            "scan",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );

    assert_ok(&output, &env);
    assert_eq!(env["data"]["summary"]["surface_count"], Value::from(3));
    assert!(
        !root.path().join("state").exists(),
        "scan must not write state"
    );

    let root_agents = surface_with_kind(&env, "agents_md", "AGENTS.md");
    assert_eq!(root_agents["scope"], Value::from("workspace"));
    assert_eq!(root_agents["agent"], Value::from("codex"));
    assert_eq!(root_agents["always_on"], Value::Bool(true));
    assert_eq!(
        root_agents["contains_skill_like_workflow"],
        Value::Bool(true)
    );

    let nested = surface_with_kind(&env, "agents_md", "crates/app/AGENTS.md");
    assert_eq!(nested["scope"], Value::from("nested"));

    let cursor = surface_with_kind(&env, "cursor_rule", ".cursor/rules/security.mdc");
    assert_eq!(cursor["agent"], Value::from("cursor"));
}

#[test]
fn instruction_show_and_classify_resolve_discovered_surface() {
    let root = TestDir::new("instruction-show-root");
    let workspace = TestDir::new("instruction-show-workspace");
    write_file(
        &workspace.path().join("AGENTS.md"),
        "# Agents\n\nUse when a repo needs lint and test workflow notes.\n",
    );

    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (scan_output, scan_env) = run_loom(
        root.path(),
        &["instruction", "scan", "--workspace", workspace_arg],
    );
    assert_ok(&scan_output, &scan_env);
    let instruction_id = scan_env["data"]["surfaces"][0]["instruction_id"]
        .as_str()
        .expect("instruction id");

    let (show_output, show_env) = run_loom(
        root.path(),
        &[
            "instruction",
            "show",
            instruction_id,
            "--workspace",
            workspace_arg,
        ],
    );
    assert_ok(&show_output, &show_env);
    assert_eq!(
        show_env["data"]["surface"]["instruction_id"],
        Value::from(instruction_id)
    );

    let (classify_output, classify_env) = run_loom_in_cwd(
        root.path(),
        workspace.path(),
        &["instruction", "classify", "AGENTS.md"],
    );
    assert_ok(&classify_output, &classify_env);
    assert_eq!(
        classify_env["data"]["surface"]["kind"],
        Value::from("agents_md")
    );
}

#[test]
fn instruction_scan_handles_deep_agents_and_copilot_instruction_scopes() {
    let root = TestDir::new("instruction-copilot-root");
    let workspace = TestDir::new("instruction-copilot-workspace");
    write_file(
        &workspace.path().join("a/b/c/d/e/f/g/AGENTS.md"),
        "# Deep Agents\n\nWorkflow steps apply deeply.\n",
    );
    write_file(
        &workspace
            .path()
            .join(".github/instructions/typescript.instructions.md"),
        "---\napplyTo: \"**/*.ts,**/*.tsx\"\n---\nUse strict TypeScript test workflow.\n",
    );

    let (deep_output, deep_env) = run_loom(
        root.path(),
        &[
            "instruction",
            "scan",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );
    assert_ok(&deep_output, &deep_env);
    let deep = surface_with_kind(&deep_env, "agents_md", "a/b/c/d/e/f/g/AGENTS.md");
    assert_eq!(deep["scope"], Value::from("nested"));

    let (copilot_output, copilot_env) = run_loom(
        root.path(),
        &[
            "instruction",
            "scan",
            "--agent",
            "copilot",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );
    assert_ok(&copilot_output, &copilot_env);
    let copilot_agents = surface_with_kind_agent(
        &copilot_env,
        "agents_md",
        "copilot",
        "a/b/c/d/e/f/g/AGENTS.md",
    );
    assert_eq!(copilot_agents["agent"], Value::from("copilot"));

    let path_specific = surface_with_kind_agent(
        &copilot_env,
        "copilot_instruction",
        "copilot",
        ".github/instructions/typescript.instructions.md",
    );
    assert_eq!(path_specific["scope"], Value::from("path-specific"));
    assert_eq!(path_specific["always_on"], Value::Bool(false));
    assert_eq!(path_specific["path_patterns"][0], Value::from("**/*.ts"));
}

#[test]
fn instruction_doctor_reports_duplicate_skill_guidance() {
    let root = TestDir::new("instruction-doctor-root");
    let workspace = TestDir::new("instruction-doctor-workspace");
    write_skill(
        root.path(),
        "fixflow",
        "---\nname: fixflow\ndescription: Use when CI test lint workflow checks must guard a code change.\n---\n# Fixflow\n\nRun CI tests and lint before merge.\n",
    );
    write_file(
        &workspace.path().join("AGENTS.md"),
        "# Agents\n\nWorkflow steps: run CI tests and lint before merge.\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "instruction",
            "doctor",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
            "--skill",
            "fixflow",
        ],
    );

    assert_ok(&output, &env);
    let findings = env["data"]["findings"].as_array().expect("findings array");
    assert!(findings.iter().any(|finding| {
        finding["id"].as_str() == Some("duplicate_guidance")
            && finding["skill"].as_str() == Some("fixflow")
    }));
    assert!(findings.iter().any(|finding| {
        finding["id"].as_str() == Some("shadowing_risk")
            && finding["affected_skill"].as_str() == Some("fixflow")
    }));
}

#[test]
fn instruction_doctor_handles_legacy_skill_entrypoint_and_bidirectional_conflicts() {
    let root = TestDir::new("instruction-doctor-legacy-root");
    let workspace = TestDir::new("instruction-doctor-legacy-workspace");
    write_file(
        &root.path().join("skills/legacy-flow/skill.md"),
        "---\nname: legacy-flow\ndescription: Use when a legacy flow must never test release changes.\n---\n# Legacy\n\nNever test release changes.\n",
    );
    write_file(
        &workspace.path().join("AGENTS.md"),
        "# Agents\n\nWorkflow steps: test release changes before merge.\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &[
            "instruction",
            "doctor",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
            "--skill",
            "legacy-flow",
        ],
    );

    assert_ok(&output, &env);
    let findings = env["data"]["findings"].as_array().expect("findings array");
    assert!(findings.iter().any(|finding| {
        finding["id"].as_str() == Some("conflicting_guidance")
            && finding["skill"].as_str() == Some("legacy-flow")
    }));
}

#[test]
fn instruction_migrate_plan_is_dry_run_and_writes_no_files() {
    let root = TestDir::new("instruction-migrate-root");
    let workspace = TestDir::new("instruction-migrate-workspace");
    write_file(
        &workspace.path().join("AGENTS.md"),
        "# Agents\n\nWorkflow steps: run cargo test before release.\n",
    );

    let (scan_output, scan_env) =
        run_loom_in_cwd(root.path(), workspace.path(), &["instruction", "scan"]);
    assert_ok(&scan_output, &scan_env);
    let instruction_id = scan_env["data"]["surfaces"][0]["instruction_id"]
        .as_str()
        .expect("instruction id");

    let (plan_output, plan_env) = run_loom_in_cwd(
        root.path(),
        workspace.path(),
        &[
            "instruction",
            "migrate-plan",
            instruction_id,
            "--to",
            "skill",
            "--name",
            "extracted-flow",
            "--dry-run",
        ],
    );

    assert_ok(&plan_output, &plan_env);
    assert_eq!(plan_env["data"]["dry_run"], Value::Bool(true));
    assert_eq!(
        plan_env["data"]["plan"]["action"],
        Value::from("extract-skill")
    );
    assert_eq!(
        plan_env["data"]["plan"]["writes"]
            .as_array()
            .expect("writes")
            .len(),
        0
    );
    assert!(!workspace.path().join("skills/extracted-flow").exists());
    assert!(!root.path().join("skills/extracted-flow").exists());
}

#[test]
fn instruction_migrate_plan_defaults_to_portable_skill_name() {
    let root = TestDir::new("instruction-migrate-name-root");
    let workspace = TestDir::new("instruction-migrate-name-workspace");
    write_file(
        &workspace
            .path()
            .join(".github/instructions/foo.instructions.md"),
        "---\napplyTo: \"**/*.rs\"\n---\nUse Rust release workflow.\n",
    );

    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (scan_output, scan_env) = run_loom(
        root.path(),
        &["instruction", "scan", "--workspace", workspace_arg],
    );
    assert_ok(&scan_output, &scan_env);
    let surface = surface_with_kind(
        &scan_env,
        "copilot_instruction",
        ".github/instructions/foo.instructions.md",
    );
    let instruction_id = surface["instruction_id"].as_str().expect("instruction id");

    let (plan_output, plan_env) = run_loom(
        root.path(),
        &[
            "instruction",
            "migrate-plan",
            instruction_id,
            "--workspace",
            workspace_arg,
            "--to",
            "skill",
            "--dry-run",
        ],
    );

    assert_ok(&plan_output, &plan_env);
    assert_eq!(
        plan_env["data"]["plan"]["would_write"][0]["path"],
        Value::from("skills/foo-instructions/SKILL.md")
    );
}

#[test]
fn instruction_scan_reports_unsupported_agent_metadata() {
    let root = TestDir::new("instruction-unsupported-root");
    let workspace = TestDir::new("instruction-unsupported-workspace");
    write_file(&workspace.path().join("AGENTS.md"), "# Agents\n");

    let (output, env) = run_loom(
        root.path(),
        &[
            "instruction",
            "scan",
            "--agent",
            "goose",
            "--workspace",
            workspace.path().to_str().expect("workspace path"),
        ],
    );

    assert_ok(&output, &env);
    assert_eq!(env["data"]["summary"]["surface_count"], Value::from(0));
    assert_eq!(
        env["data"]["unsupported_surfaces"][0]["agent"],
        Value::from("goose")
    );
}
