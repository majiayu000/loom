mod common;

use serde_json::Value;

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
