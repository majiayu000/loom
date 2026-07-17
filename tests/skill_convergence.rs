mod common;
#[path = "skill_convergence/executor.rs"]
mod skill_convergence_executor;

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
    projected_fixture_with_method("copy")
}

fn projected_fixture_with_method(method: &str) -> Fixture {
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
    let (output, env) = skill_project(root.path(), "demo", binding_id, Some(method));
    assert!(output.status.success(), "project failed: {env}");
    Fixture {
        root,
        workspace,
        target,
    }
}

fn add_copy_projection(fixture: &Fixture, suffix: &str) -> (std::path::PathBuf, String) {
    let target_path = fixture.root.path().join(format!("live/{suffix}"));
    let (output, target) = target_add(fixture.root.path(), "claude", &target_path, "managed");
    assert!(output.status.success(), "target add failed: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, binding) = binding_add(
        fixture.root.path(),
        "claude",
        "default",
        "exact-path",
        workspace,
        target_id,
    );
    assert!(output.status.success(), "binding add failed: {binding}");
    let binding_id = binding["data"]["binding"]["binding_id"]
        .as_str()
        .expect("binding id");
    let (output, projection) = skill_project(fixture.root.path(), "demo", binding_id, Some("copy"));
    assert!(output.status.success(), "project failed: {projection}");
    let instance_id = projection["data"]["projection"]["instance_id"]
        .as_str()
        .expect("instance id")
        .to_string();
    (target_path.join("demo"), instance_id)
}

fn conflict_codes(plan: &Value) -> Vec<&str> {
    plan["data"]["conflicts"]
        .as_array()
        .expect("conflicts")
        .iter()
        .filter_map(|conflict| conflict["code"].as_str())
        .collect()
}

fn mutate_plan_event(root: &Path, plan_id: &str, mut mutate: impl FnMut(&mut Value)) {
    let audit_path = root.join("state/events/commands.jsonl");
    let raw = fs::read_to_string(&audit_path).expect("read command events");
    let mut changed = false;
    let rows = raw
        .lines()
        .map(|line| {
            let mut event: Value = serde_json::from_str(line).expect("parse command event");
            if event["cmd"] == json!("plan.converge")
                && event["status"] == json!("succeeded")
                && event["output"]["plan_id"] == json!(plan_id)
            {
                mutate(&mut event["output"]);
                changed = true;
            }
            serde_json::to_string(&event).expect("serialize command event")
        })
        .collect::<Vec<_>>();
    assert!(changed, "expected stored convergence plan event");
    fs::write(&audit_path, format!("{}\n", rows.join("\n"))).expect("rewrite stored plan");
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
    assert_eq!(first["data"]["schema_version"], json!("1.2"));
    assert_eq!(first["data"]["operation"], json!("converge"));
    assert_eq!(first["data"]["safe_to_apply"], json!(true));
    assert_eq!(first["data"]["execution_enabled"], json!(true));
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
            .is_some_and(|versions| versions.contains(&json!("1.2"))),
        "authoritative schema must declare convergence schema 1.2"
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

    let (output, applied) = run_loom(
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
    assert!(output.status.success(), "reviewed plan failed: {applied}");
    assert_ne!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        domain_before
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
    assert_ne!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        head_before,
        "successful convergence must commit updated projection registry state"
    );

    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["selectors"]["profile"] = json!("tampered");
    });

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
fn convergence_event_kind_cannot_bypass_digest_confirmation() {
    let fixture = projected_fixture();
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, use_plan) = run_loom(
        fixture.root.path(),
        &[
            "plan",
            "use",
            "demo",
            "--agents",
            "claude",
            "--workspace",
            workspace,
            "--method",
            "copy",
        ],
    );
    assert!(output.status.success(), "use plan failed: {use_plan}");

    let (output, convergence) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "convergence plan failed: {convergence}"
    );
    let plan_id = convergence["data"]["plan_id"].as_str().expect("plan id");
    let digest = convergence["data"]["plan_digest"]
        .as_str()
        .expect("plan digest");
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        stored["requires_digest_confirmation"] = json!(false);
        stored["use_args"] = use_plan["data"]["use_args"].clone();
        stored["guards"] = use_plan["data"]["guards"].clone();
    });

    let target_before = snapshot_tree(fixture.target.path());
    let (output, corrupt) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "conv-kind-bypass",
        ],
    );
    assert!(!output.status.success(), "kind bypass passed: {corrupt}");
    assert_eq!(corrupt["error"]["code"], json!("STATE_CORRUPT"));
    assert_eq!(
        corrupt["error"]["details"]["conflict"]["code"],
        json!("PLAN_CORRUPT")
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
}

#[test]
fn convergence_apply_reports_idempotency_key_conflict_before_executor_block() {
    let fixture = projected_fixture();
    let workspace = fixture.workspace.path().to_str().expect("workspace path");
    let (output, use_plan) = run_loom(
        fixture.root.path(),
        &[
            "plan",
            "use",
            "demo",
            "--agents",
            "claude",
            "--workspace",
            workspace,
            "--method",
            "copy",
        ],
    );
    assert!(output.status.success(), "use plan failed: {use_plan}");
    let use_id = use_plan["data"]["plan_id"].as_str().expect("use plan id");
    let (output, applied) = run_loom(
        fixture.root.path(),
        &["apply", use_id, "--idempotency-key", "shared-key"],
    );
    assert!(output.status.success(), "use apply failed: {applied}");

    let (output, convergence) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "convergence plan failed: {convergence}"
    );
    let plan_id = convergence["data"]["plan_id"].as_str().expect("plan id");
    let digest = convergence["data"]["plan_digest"]
        .as_str()
        .expect("plan digest");
    let (output, conflict) = run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            "shared-key",
        ],
    );
    assert!(!output.status.success(), "key conflict passed: {conflict}");
    assert_eq!(conflict["error"]["code"], json!("DEPENDENCY_CONFLICT"));
    assert_eq!(
        conflict["error"]["details"]["conflict"]["code"],
        json!("IDEMPOTENCY_KEY_REUSED")
    );
}

#[test]
fn projection_input_requires_instance() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "copy plan failed: {plan}");
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id")
        .to_string();
    assert_eq!(plan["data"]["source"]["direction"], json!("source"));

    let (output, missing_selector) = plan_converge(&fixture, &["--from-projection"]);
    assert!(
        !output.status.success(),
        "projection direction without instance passed: {missing_selector}"
    );

    let (output, unknown) = plan_converge(
        &fixture,
        &["--from-projection", "--instance", "inst_unknown"],
    );
    assert!(
        !output.status.success(),
        "unknown instance passed: {unknown}"
    );
    assert_eq!(unknown["error"]["code"], json!("PROJECTION_CONFLICT"));

    let (output, selected) =
        plan_converge(&fixture, &["--from-projection", "--instance", &instance]);
    assert!(output.status.success(), "selected input failed: {selected}");
    assert_eq!(selected["data"]["source"]["direction"], json!("projection"));
    assert_eq!(
        selected["data"]["input"]["selected_projection_instance"],
        json!(instance)
    );
    assert_eq!(
        selected["data"]["input"]["selected_input_tree_digest"],
        selected["data"]["preflight"]["input_tree_digest"]
    );

    fs::write(
        fixture.target.path().join("demo/SKILL.md"),
        "---\nname: wrong-name\n---\n# invalid projection input\n",
    )
    .expect("write invalid projection input");
    let (output, blocked) =
        plan_converge(&fixture, &["--from-projection", "--instance", &instance]);
    assert!(
        output.status.success(),
        "blocked input must remain reviewable: {blocked}"
    );
    assert_eq!(
        blocked["data"]["preflight"]["input_direction"],
        json!("projection")
    );
    assert!(
        !blocked["data"]["preflight"]["mutation_allowed"]
            .as_bool()
            .expect("mutation_allowed")
    );
    assert!(conflict_codes(&blocked).contains(&"SOURCE_PREFLIGHT_BLOCKED"));

    let (output, source_default) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "source default failed: {source_default}"
    );
    assert_eq!(
        source_default["data"]["source"]["direction"],
        json!("source")
    );
    assert_eq!(
        source_default["data"]["preflight"]["input_direction"],
        json!("source")
    );

    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove copy projection");
    let (output, missing) =
        plan_converge(&fixture, &["--from-projection", "--instance", &instance]);
    assert!(
        !output.status.success(),
        "missing projection passed: {missing}"
    );
    assert_eq!(missing["error"]["code"], json!("PROJECTION_CONFLICT"));
    fs::write(fixture.target.path().join("demo"), "not a directory")
        .expect("replace projection with file");
    let (output, not_directory) =
        plan_converge(&fixture, &["--from-projection", "--instance", &instance]);
    assert!(
        !output.status.success(),
        "non-directory projection passed: {not_directory}"
    );
    assert_eq!(not_directory["error"]["code"], json!("PROJECTION_CONFLICT"));

    let symlink = projected_fixture_with_method("symlink");
    let (output, plan) = plan_converge(&symlink, &[]);
    assert!(output.status.success(), "symlink plan failed: {plan}");
    assert!(
        plan["data"]["effects"][0]["materialized_tree_digest"].is_null(),
        "symlink observation must not hash followed source bytes: {plan}"
    );
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance id");
    let (output, rejected) =
        plan_converge(&symlink, &["--from-projection", "--instance", instance]);
    assert!(
        !output.status.success(),
        "symlink projection input passed: {rejected}"
    );
    assert_eq!(rejected["error"]["code"], json!("PROJECTION_CONFLICT"));

    let materialize = projected_fixture_with_method("materialize");
    let (output, plan) = plan_converge(&materialize, &[]);
    assert!(output.status.success(), "materialize plan failed: {plan}");
    let instance = plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("materialize instance");
    let (output, selected) =
        plan_converge(&materialize, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "materialize input failed: {selected}"
    );
    assert_eq!(
        selected["data"]["input"]["projections"][0]["method"],
        json!("materialize")
    );
}

#[test]
fn dirty_side_conflicts() {
    let fixture = projected_fixture();
    let (first_path, first_instance) = {
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "initial plan failed: {plan}");
        (
            fixture.target.path().join("demo"),
            plan["data"]["effects"][0]["instance_id"]
                .as_str()
                .expect("first instance")
                .to_string(),
        )
    };
    let (second_path, second_instance) = add_copy_projection(&fixture, "second-copy");
    write_skill(
        fixture.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing dirty source convergence.\n---\n# source v2\n",
    );
    fs::write(
        first_path.join("SKILL.md"),
        "---\nname: demo\ndescription: Use when testing first dirty projection.\n---\n# projection a\n",
    )
    .expect("edit first projection");
    fs::write(
        second_path.join("SKILL.md"),
        "---\nname: demo\ndescription: Use when testing second dirty projection.\n---\n# projection b\n",
    )
    .expect("edit second projection");

    let (output, conflicted) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "dirty conflict plan failed: {conflicted}"
    );
    assert_eq!(
        conflict_codes(&conflicted),
        vec![
            "SOURCE_PROJECTION_DIRTY_CONFLICT",
            "DIVERGENT_PROJECTION_INPUTS",
            "MULTIPLE_DIRTY_PROJECTION_INPUTS"
        ]
    );
    let items = conflicted["data"]["input"]["projections"]
        .as_array()
        .expect("projection evidence");
    assert_eq!(items.len(), 2);
    assert!(
        items
            .windows(2)
            .all(|pair| { pair[0]["instance_id"].as_str() < pair[1]["instance_id"].as_str() })
    );
    assert!(items.iter().all(|item| {
        item["state"] == json!("dirty")
            && item["baseline_tree_digest"].as_str().is_some()
            && item["live_tree_digest"].as_str().is_some()
    }));
    let first_digest = conflicted["data"]["plan_digest"].clone();
    fs::write(
        second_path.join("SKILL.md"),
        "---\nname: demo\ndescription: Use when testing changed evidence.\n---\n# projection c\n",
    )
    .expect("change second projection again");
    let (output, changed) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "changed evidence plan failed: {changed}"
    );
    assert_ne!(changed["data"]["plan_digest"], first_digest);

    let same = projected_fixture();
    let (output, initial) = plan_converge(&same, &[]);
    assert!(
        output.status.success(),
        "same initial plan failed: {initial}"
    );
    let selected = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("selected instance")
        .to_string();
    let first = same.target.path().join("demo/SKILL.md");
    let (second, _) = add_copy_projection(&same, "same-copy");
    let body = "---\nname: demo\ndescription: Use when testing identical dirty projections.\n---\n# same dirty bytes\n";
    fs::write(first, body).expect("edit same first");
    fs::write(second.join("SKILL.md"), body).expect("edit same second");
    let (output, selected_plan) =
        plan_converge(&same, &["--from-projection", "--instance", &selected]);
    assert!(
        output.status.success(),
        "explicit identical input failed: {selected_plan}"
    );
    assert!(!conflict_codes(&selected_plan).contains(&"DIVERGENT_PROJECTION_INPUTS"));
    assert_eq!(
        selected_plan["data"]["input"]["selected_projection_instance"],
        json!(selected)
    );
    let (output, ambiguous) = plan_converge(&same, &[]);
    assert!(
        output.status.success(),
        "identical dirty plan failed: {ambiguous}"
    );
    assert!(conflict_codes(&ambiguous).contains(&"MULTIPLE_DIRTY_PROJECTION_INPUTS"));

    let projections_path = same.root.path().join("state/registry/projections.json");
    let mut projections: Value =
        serde_json::from_slice(&fs::read(&projections_path).expect("read projections state"))
            .expect("parse projections state");
    projections["projections"][0]["last_applied_rev"] = json!("missing-baseline-ref");
    fs::write(
        &projections_path,
        serde_json::to_vec_pretty(&projections).expect("serialize projections state"),
    )
    .expect("write projections state");
    let (output, unavailable) = plan_converge(&same, &[]);
    assert!(
        output.status.success(),
        "baseline conflict plan failed: {unavailable}"
    );
    assert!(conflict_codes(&unavailable).contains(&"PROJECTION_EVIDENCE_UNAVAILABLE"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let unreadable_path = same.target.path().join("demo/SKILL.md");
        let original_mode = fs::metadata(&unreadable_path)
            .expect("unreadable fixture metadata")
            .permissions()
            .mode();
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(0o000))
            .expect("make projection unreadable");
        let (output, unreadable) = plan_converge(&same, &[]);
        fs::set_permissions(&unreadable_path, fs::Permissions::from_mode(original_mode))
            .expect("restore projection permissions");
        assert!(
            output.status.success(),
            "unreadable conflict plan failed: {unreadable}"
        );
        assert!(conflict_codes(&unreadable).contains(&"PROJECTION_EVIDENCE_UNAVAILABLE"));
        assert!(
            unreadable["data"]["input"]["projections"]
                .as_array()
                .expect("unreadable evidence")
                .iter()
                .any(|item| item["state"] == json!("unreadable"))
        );
    }

    assert_ne!(first_instance, second_instance);
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
