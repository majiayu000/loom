mod common;

use serde_json::Value;

use common::actions::target_add;
use common::{TestDir, run_loom, write_file};

fn write_skill_file(root: &TestDir, skill: &str, file_name: &str, body: &str) {
    write_file(
        &root.path().join("skills").join(skill).join(file_name),
        body,
    );
}

fn report(env: &Value) -> &Value {
    if env["ok"] == Value::Bool(true) {
        &env["data"]
    } else {
        &env["error"]["details"]["report"]
    }
}

fn has_finding(report: &Value, id: &str, severity: &str) -> bool {
    report["findings"]
        .as_array()
        .expect("findings array")
        .iter()
        .any(|finding| {
            finding["id"].as_str() == Some(id) && finding["severity"].as_str() == Some(severity)
        })
}

#[test]
fn skill_lint_accepts_valid_strict_skill() {
    let root = TestDir::new("skill-lint-valid");
    write_skill_file(
        &root,
        "portable-skill",
        "SKILL.md",
        "---\nname: portable-skill\ndescription: Use when an agent needs portable skill metadata linting before projection.\n---\n# Portable\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "portable-skill", "--strict"],
    );

    assert!(output.status.success(), "strict lint should pass");
    let report = report(&env);
    assert_eq!(report["valid"], Value::Bool(true));
    assert_eq!(report["summary"]["error_count"], Value::from(0));
    assert_eq!(report["entrypoint"]["file_name"], Value::from("SKILL.md"));
}

#[test]
fn skill_lint_compat_warns_for_lowercase_entrypoint() {
    let root = TestDir::new("skill-lint-legacy");
    write_skill_file(
        &root,
        "legacy-skill",
        "skill.md",
        "---\nname: legacy-skill\ndescription: Use when an older agent skill still ships a lowercase entrypoint.\n---\n# Legacy\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "legacy-skill", "--compat"]);

    assert!(
        output.status.success(),
        "compat lint should allow legacy entrypoint"
    );
    let report = report(&env);
    assert_eq!(report["compatible"], Value::Bool(true));
    assert!(has_finding(report, "entrypoint_case", "warning"));
}

#[test]
fn skill_lint_rejects_invalid_yaml_frontmatter() {
    let root = TestDir::new("skill-lint-yaml");
    write_skill_file(
        &root,
        "bad-yaml",
        "SKILL.md",
        "---\nname: [bad\n---\n# Bad\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "bad-yaml"]);

    assert!(
        !output.status.success(),
        "invalid YAML should fail strict lint"
    );
    assert_eq!(env["error"]["code"], Value::from("SCHEMA_MISMATCH"));
    assert!(has_finding(
        report(&env),
        "frontmatter_yaml_invalid",
        "error"
    ));
}

#[test]
fn skill_lint_rejects_invalid_frontmatter_name() {
    let root = TestDir::new("skill-lint-name");
    write_skill_file(
        &root,
        "bad-name",
        "SKILL.md",
        "---\nname: Bad_Name\ndescription: Use when an agent needs to catch invalid portable skill names.\n---\n# Bad name\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "bad-name"]);

    assert!(
        !output.status.success(),
        "invalid name should fail strict lint"
    );
    assert!(has_finding(report(&env), "name_invalid", "error"));
}

#[test]
fn skill_lint_rejects_missing_description() {
    let root = TestDir::new("skill-lint-description");
    write_skill_file(
        &root,
        "missing-description",
        "SKILL.md",
        "---\nname: missing-description\n---\n# Missing description\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "missing-description"]);

    assert!(
        !output.status.success(),
        "missing description should fail strict lint"
    );
    assert!(has_finding(report(&env), "description_missing", "error"));
}

#[test]
fn skill_lint_rejects_frontmatter_directory_mismatch() {
    let root = TestDir::new("skill-lint-mismatch");
    write_skill_file(
        &root,
        "actual-name",
        "SKILL.md",
        "---\nname: other-name\ndescription: Use when an agent needs to detect mismatched skill identity metadata.\n---\n# Mismatch\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "actual-name"]);

    assert!(
        !output.status.success(),
        "name mismatch should fail strict lint"
    );
    assert!(has_finding(
        report(&env),
        "name_directory_mismatch",
        "error"
    ));
}

#[test]
fn skill_lint_portable_accepts_rich_yaml_frontmatter() {
    let root = TestDir::new("skill-lint-rich-yaml");
    write_skill_file(
        &root,
        "rich-skill",
        "SKILL.md",
        "---\nname: rich-skill\ndescription: Use when an agent needs rich portable YAML metadata before registry projection.\nlicense: MIT\ncompatibility:\n  runtimes:\n    - codex\nmetadata:\n  owner: platform\n  risk: low\nallowed-tools: Bash, Read\n---\n# Rich skill\n",
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "rich-skill", "--portable"]);

    assert!(
        output.status.success(),
        "portable lint should accept nested YAML frontmatter: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = report(&env);
    assert_eq!(report["valid"], Value::Bool(true));
    assert_eq!(report["frontmatter"]["license"], Value::from("MIT"));
    assert_eq!(
        report["frontmatter"]["metadata"]["owner"],
        Value::from("platform")
    );
    assert_eq!(
        report["sections"]["portable_spec"]["status"],
        Value::from("pass")
    );
}

#[test]
fn skill_lint_agent_codex_warns_for_claude_only_fields() {
    let root = TestDir::new("skill-lint-agent-codex");
    write_skill_file(
        &root,
        "agent-skill",
        "SKILL.md",
        "---\nname: agent-skill\ndescription: Use when an agent needs target compatibility checks before activation.\nallowed-tools: Bash, Read\n---\n# Agent skill\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "agent-skill", "--agent", "codex"],
    );

    assert!(
        output.status.success(),
        "agent warning should not fail lint"
    );
    let report = report(&env);
    assert!(has_finding(
        report,
        "agent_codex_unsupported_field",
        "warning"
    ));
    assert_eq!(
        report["sections"]["agent_compatibility"]["codex"]["status"],
        Value::from("warning")
    );
}

#[test]
fn skill_lint_agent_claude_accepts_claude_fields() {
    let root = TestDir::new("skill-lint-agent-claude");
    write_skill_file(
        &root,
        "claude-skill",
        "SKILL.md",
        "---\nname: claude-skill\ndescription: Use when Claude needs target compatibility checks before activation.\nallowed-tools: Bash, Read\n---\n# Agent skill\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "claude-skill", "--agent", "claude"],
    );

    assert!(
        output.status.success(),
        "Claude-specific fields should not warn for Claude"
    );
    let report = report(&env);
    assert_eq!(
        report["sections"]["agent_compatibility"]["claude"]["status"],
        Value::from("pass")
    );
    assert_eq!(report["frontmatter"]["agent_fields"][0], "allowed-tools");
}

#[test]
fn skill_lint_agent_reports_active_skill_name_collision() {
    let root = TestDir::new("skill-lint-agent-collision");
    let active_dir = root.path().join("active-codex");
    write_file(
        &root.path().join(".env"),
        &format!("CODEX_SKILLS_DIR={}\n", active_dir.display()),
    );
    write_skill_file(
        &root,
        "collision-skill",
        "SKILL.md",
        "---\nname: collision-skill\ndescription: Use when Codex needs collision checks before activation.\n---\n# Source\n",
    );
    write_file(
        &active_dir.join("collision-skill/SKILL.md"),
        "---\nname: collision-skill\ndescription: Use when a stale active copy can shadow the source skill.\n---\n# Active\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "collision-skill", "--agent", "codex"],
    );

    assert!(
        output.status.success(),
        "collision warning should not fail lint"
    );
    let report = report(&env);
    assert!(has_finding(report, "agent_skill_name_collision", "warning"));
    assert_eq!(
        report["sections"]["agent_compatibility"]["codex"]["status"],
        Value::from("warning")
    );
}

#[test]
fn skill_lint_agent_reports_registered_target_name_collision() {
    let root = TestDir::new("skill-lint-registered-target-collision");
    let active_dir = root.path().join("registered-codex");
    let (target_output, target_env) = target_add(root.path(), "codex", &active_dir, "managed");
    assert!(
        target_output.status.success(),
        "target add should pass: {target_env}"
    );
    write_skill_file(
        &root,
        "registered-collision",
        "SKILL.md",
        "---\nname: registered-collision\ndescription: Use when Codex needs registered target collision checks before activation.\n---\n# Source\n",
    );
    write_file(
        &active_dir.join("registered-collision/SKILL.md"),
        "---\nname: registered-collision\ndescription: Use when a registered active copy can shadow the source skill.\n---\n# Active\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "registered-collision", "--agent", "codex"],
    );

    assert!(
        output.status.success(),
        "collision warning should not fail lint"
    );
    let report = report(&env);
    assert!(has_finding(report, "agent_skill_name_collision", "warning"));
}

#[test]
fn skill_lint_rejects_too_long_description() {
    let root = TestDir::new("skill-lint-description-long");
    let long_description = format!(
        "Use when an agent needs portable lint validation. {}",
        "x".repeat(1030)
    );
    write_skill_file(
        &root,
        "long-description",
        "SKILL.md",
        &format!(
            "---\nname: long-description\ndescription: {}\n---\n# Long\n",
            long_description
        ),
    );

    let (output, env) = run_loom(root.path(), &["skill", "lint", "long-description"]);

    assert!(
        !output.status.success(),
        "long description should fail strict lint"
    );
    assert!(has_finding(report(&env), "description_too_long", "error"));
}

#[test]
fn skill_lint_quality_reports_eval_and_script_findings() {
    let root = TestDir::new("skill-lint-quality");
    write_skill_file(
        &root,
        "scripted-skill",
        "SKILL.md",
        "---\nname: scripted-skill\ndescription: Use when an agent needs scripted repository cleanup checks before projection.\n---\n# Scripted skill\n",
    );
    write_file(
        &root.path().join("skills/scripted-skill/scripts/run.sh"),
        "echo missing shebang\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "scripted-skill", "--quality"],
    );

    assert!(
        output.status.success(),
        "quality warnings should not fail lint"
    );
    let report = report(&env);
    assert!(has_finding(report, "quality_evals_missing", "warning"));
    assert!(has_finding(
        report,
        "quality_script_entrypoint_unclear",
        "warning"
    ));
    assert_eq!(report["sections"]["resources"]["scripts"], Value::from(1));
    assert_eq!(
        report["sections"]["quality"]["status"],
        Value::from("warning")
    );
}

#[test]
fn skill_lint_quality_reports_vague_large_and_deep_references() {
    let root = TestDir::new("skill-lint-quality-depth");
    let mut body = String::from(
        "---\nname: sprawling-skill\ndescription: Helpful assistant productivity workflow automation tasks\n---\n# Sprawling\n",
    );
    for index in 0..401 {
        body.push_str(&format!("Line {index}\n"));
    }
    write_skill_file(&root, "sprawling-skill", "SKILL.md", &body);
    write_file(
        &root
            .path()
            .join("skills/sprawling-skill/references/a/b/deep.md"),
        "Deep reference\n",
    );

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "sprawling-skill", "--compat", "--quality"],
    );

    assert!(
        output.status.success(),
        "compat quality warnings should not fail lint"
    );
    let report = report(&env);
    assert!(has_finding(report, "quality_description_vague", "warning"));
    assert!(has_finding(report, "quality_skill_md_large", "warning"));
    assert!(has_finding(report, "quality_reference_too_deep", "warning"));
}

#[cfg(unix)]
#[test]
fn skill_lint_quality_skips_symlinked_reference_directories() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("skill-lint-reference-symlink-loop");
    write_skill_file(
        &root,
        "linked-reference-skill",
        "SKILL.md",
        "---\nname: linked-reference-skill\ndescription: Use when an agent needs reference linting without following symlink loops.\n---\n# Linked references\n",
    );
    let references = root.path().join("skills/linked-reference-skill/references");
    std::fs::create_dir_all(&references).expect("create references dir");
    symlink(".", references.join("self")).expect("create self symlink");

    let (output, env) = run_loom(
        root.path(),
        &["skill", "lint", "linked-reference-skill", "--quality"],
    );

    assert!(
        output.status.success(),
        "quality lint should finish without following reference symlink loops"
    );
    let report = report(&env);
    assert!(!has_finding(
        report,
        "quality_reference_too_deep",
        "warning"
    ));
}
