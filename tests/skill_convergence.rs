mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

use common::actions::{binding_add, save_skill, skill_project, target_add};
use common::{TestDir, run_loom, write_skill};

struct Fixture {
    root: TestDir,
    workspace: TestDir,
    target: TestDir,
}

fn projected_fixture() -> Fixture {
    let root = TestDir::new("convergence-plan-root");
    let workspace = TestDir::new("convergence-plan-workspace");
    let target = TestDir::new("convergence-plan-target");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing convergence planning.\n---\n# demo\n",
    );
    let (output, env) = save_skill(root.path(), "demo");
    assert!(output.status.success(), "save skill failed: {env}");

    let (output, env) = target_add(root.path(), "claude", target.path(), "managed");
    assert!(output.status.success(), "target add failed: {env}");
    let target_id = env["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace_arg = workspace.path().to_str().expect("workspace path");
    let (output, env) = binding_add(
        root.path(),
        "claude",
        "default",
        "exact-path",
        workspace_arg,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {env}");
    let binding_id = env["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, env) = skill_project(root.path(), "demo", binding_id, Some("copy"));
    assert!(output.status.success(), "project failed: {env}");
    Fixture {
        root,
        workspace,
        target,
    }
}

fn plan_converge(fixture: &Fixture, extra: &[&str]) -> (std::process::Output, Value) {
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let mut args = vec![
        "plan",
        "converge",
        "demo",
        "--agent",
        "claude",
        "--workspace",
        workspace,
        "--profile",
        "default",
    ];
    args.extend_from_slice(extra);
    run_loom(fixture.root.path(), &args)
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output utf8")
}

fn snapshot_tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn visit(base: &Path, path: &Path, files: &mut BTreeMap<String, Vec<u8>>) {
        if !path.exists() {
            return;
        }
        for entry in fs::read_dir(path).expect("read snapshot directory") {
            let entry = entry.expect("read snapshot entry");
            let child = entry.path();
            if entry.file_type().expect("snapshot file type").is_dir() {
                visit(base, &child, files);
            } else {
                let relative = child
                    .strip_prefix(base)
                    .expect("relative snapshot path")
                    .display()
                    .to_string();
                files.insert(relative, fs::read(&child).expect("read snapshot file"));
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn exact_effect_plan() {
    let fixture = projected_fixture();
    let (output, first) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "first plan failed: {first}");
    let (output, second) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "second plan failed: {second}");

    assert_eq!(first["cmd"], json!("plan.converge"));
    assert_eq!(first["data"]["protocol_version"], json!("1.0"));
    assert_eq!(first["data"]["schema_version"], json!("1.1"));
    assert_eq!(first["data"]["operation"], json!("converge"));
    assert_eq!(first["data"]["safe_to_apply"], json!(false));
    assert_eq!(first["data"]["execution_enabled"], json!(false));
    assert_eq!(first["data"]["requires_digest_confirmation"], json!(true));
    assert!(first["data"]["next_actions"].is_null());
    assert_ne!(first["data"]["plan_id"], second["data"]["plan_id"]);
    assert_eq!(first["data"]["plan_digest"], second["data"]["plan_digest"]);
    assert_eq!(first["data"]["effects"], second["data"]["effects"]);

    let effects = first["data"]["effects"].as_array().expect("effects array");
    assert_eq!(
        effects.len(),
        1,
        "plan must not broaden selected scope: {first}"
    );
    assert_eq!(effects[0]["agent"], json!("claude"));
    assert_eq!(effects[0]["profile"], json!("default"));
    assert_eq!(effects[0]["method"], json!("copy"));
    assert_eq!(effects[0]["ownership"], json!("managed"));
    assert_eq!(first["data"]["required_axes"], json!(["projections"]));

    let schema: Value =
        serde_json::from_str(include_str!("../docs/schemas/agent-plan-v1.schema.json"))
            .expect("agent plan schema JSON");
    assert!(
        schema["properties"]["schema_version"]["enum"]
            .as_array()
            .is_some_and(|versions| versions.contains(&json!("1.1"))),
        "authoritative schema must declare convergence schema 1.1"
    );
    assert!(
        schema["properties"]["operation"]["enum"]
            .as_array()
            .is_some_and(|operations| operations.contains(&json!("converge"))),
        "authoritative schema must declare plan converge"
    );
}

#[test]
fn plan_only_writes_plan_and_audit() {
    let fixture = projected_fixture();
    let registry_before = snapshot_tree(&fixture.root.path().join("state/registry"));
    let source_before = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let target_before = snapshot_tree(fixture.target.path());
    let head_before = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let status_before = git(fixture.root.path(), &["status", "--porcelain"]);
    let audit_path = fixture.root.path().join("state/events/commands.jsonl");
    let audit_before = fs::read_to_string(&audit_path).expect("read audit before");

    let (output, env) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {env}");

    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry_before
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source_before
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before
    );
    assert_eq!(
        git(fixture.root.path(), &["status", "--porcelain"]),
        status_before
    );
    let audit_after = fs::read_to_string(audit_path).expect("read audit after");
    assert!(
        audit_after.len() > audit_before.len(),
        "plan must append durable audit"
    );
    assert!(audit_after.contains("plan.converge"));
}

#[test]
fn apply_requires_reviewed_plan_digest() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let digest = plan["data"]["plan_digest"].as_str().expect("plan digest");
    let domain_before = snapshot_tree(&fixture.root.path().join("state/registry"));
    let target_before = snapshot_tree(fixture.target.path());
    let head_before = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let (output, missing) = run_loom(
        fixture.root.path(),
        &["apply", plan_id, "--idempotency-key", "conv-missing"],
    );
    assert!(
        !output.status.success(),
        "missing digest unexpectedly passed: {missing}"
    );
    assert_eq!(
        missing["error"]["details"]["conflict"]["code"],
        json!("PLAN_DIGEST_REQUIRED")
    );

    let (output, mismatch) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            "sha256:wrong",
            "--idempotency-key",
            "conv-mismatch",
        ],
    );
    assert!(
        !output.status.success(),
        "mismatch unexpectedly passed: {mismatch}"
    );
    assert_eq!(mismatch["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        mismatch["error"]["details"]["conflict"]["code"],
        json!("PLAN_DIGEST_MISMATCH")
    );

    let (output, disabled) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "conv-reviewed",
        ],
    );
    assert!(
        !output.status.success(),
        "planning tranche executed: {disabled}"
    );
    assert_eq!(disabled["error"]["code"], json!("POLICY_BLOCKED"));
    assert_eq!(
        disabled["error"]["details"]["conflict"]["code"],
        json!("CONVERGENCE_EXECUTOR_UNAVAILABLE")
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        domain_before
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before
    );

    let audit_path = fixture.root.path().join("state/events/commands.jsonl");
    let raw = fs::read_to_string(&audit_path).expect("read convergence audit");
    let mut tampered = false;
    let rows = raw
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("parse command event");
            if event["cmd"] == json!("plan.converge")
                && event["status"] == json!("succeeded")
                && event["output"]["plan_id"] == json!(plan_id)
            {
                event["output"]["selectors"]["profile"] = json!("tampered");
                tampered = true;
            }
            serde_json::to_string(&event).expect("serialize command event")
        })
        .collect::<Vec<_>>();
    assert!(tampered, "expected stored convergence plan event");
    fs::write(&audit_path, format!("{}\n", rows.join("\n"))).expect("tamper stored plan");

    let (output, corrupt) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "conv-corrupt",
        ],
    );
    assert!(!output.status.success(), "corrupt plan passed: {corrupt}");
    assert_eq!(corrupt["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        corrupt["error"]["details"]["conflict"]["code"],
        json!("PLAN_DIGEST_INVALID")
    );
}

#[test]
fn source_only_and_required_runtime() {
    let root = TestDir::new("convergence-source-only");
    write_skill(
        root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing source-only convergence.\n---\n# demo\n",
    );
    let (output, env) = save_skill(root.path(), "demo");
    assert!(output.status.success(), "save failed: {env}");

    let (output, source_only) = run_loom(root.path(), &["plan", "converge", "demo"]);
    assert!(
        output.status.success(),
        "source-only plan failed: {source_only}"
    );
    assert_eq!(source_only["data"]["effects"], json!([]));
    assert_eq!(
        source_only["data"]["projection_state"],
        json!("not_applicable")
    );

    let (output, required) = run_loom(
        root.path(),
        &["plan", "converge", "demo", "--require-runtime"],
    );
    assert!(
        output.status.success(),
        "blocked plan should remain reviewable: {required}"
    );
    assert_eq!(required["data"]["safe_to_apply"], json!(false));
    assert_eq!(
        required["data"]["conflicts"][0]["code"],
        json!("RUNTIME_PROJECTION_REQUIRED")
    );
    assert_eq!(
        required["data"]["required_axes"],
        json!(["projections", "visibility"])
    );
}
