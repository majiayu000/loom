use std::fs;

use serde_json::{Value, json};

use super::skill_convergence_executor::apply_plan;
use super::*;

#[path = "../../src/sha256.rs"]
mod guard_sha256;

const CONVERGENCE_DIGEST_FIELDS: [&str; 13] = [
    "skill",
    "selectors",
    "source",
    "input",
    "preflight",
    "input_conflicts",
    "registry",
    "projections",
    "visibility",
    "accept_restart_required",
    "remote",
    "required_axes",
    "required_approvals",
];

fn reseal_plan_event(
    fixture: &Fixture,
    plan: &Value,
    mut mutate: impl FnMut(&mut Value),
) -> String {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    let mut resealed = None;
    mutate_plan_event(fixture.root.path(), plan_id, |stored| {
        mutate(stored);
        let object = stored.as_object().expect("stored plan object");
        let payload = CONVERGENCE_DIGEST_FIELDS
            .into_iter()
            .map(|field| (field.to_string(), object[field].clone()))
            .collect::<serde_json::Map<_, _>>();
        let mut hasher = guard_sha256::Sha256::new();
        hasher.update(&serde_json::to_vec(&payload).expect("serialize plan digest payload"));
        let digest = format!("sha256:{}", guard_sha256::to_hex(&hasher.finalize()));
        stored["plan_digest"] = json!(digest);
        resealed = stored["plan_digest"].as_str().map(str::to_string);
    });
    resealed.expect("resealed plan digest")
}

fn apply_resealed_plan(
    fixture: &Fixture,
    plan: &Value,
    digest: &str,
    key: &str,
) -> (std::process::Output, Value) {
    let plan_id = plan["data"]["plan_id"].as_str().expect("plan id");
    run_loom(
        fixture.root.path(),
        &[
            "apply",
            plan_id,
            "--plan-digest",
            digest,
            "--idempotency-key",
            key,
        ],
    )
}

fn clear_review_blockers(stored: &mut Value) {
    stored["safe_to_apply"] = json!(true);
    stored["input_conflicts"] = json!([]);
    stored["preflight"]["mutation_allowed"] = json!(true);
    stored["preflight"]["regression_ids"] = json!([]);
}

struct MutationSnapshot {
    head: String,
    source: BTreeMap<String, Vec<u8>>,
    registry: BTreeMap<String, Vec<u8>>,
    target: BTreeMap<String, Vec<u8>>,
}

fn mutation_snapshot(fixture: &Fixture) -> MutationSnapshot {
    MutationSnapshot {
        head: git(fixture.root.path(), &["rev-parse", "HEAD"]),
        source: snapshot_tree(&fixture.root.path().join("skills/demo")),
        registry: snapshot_tree(&fixture.root.path().join("state/registry")),
        target: snapshot_tree(fixture.target.path()),
    }
}

fn assert_rejected_before_writes(fixture: &Fixture, before: MutationSnapshot) {
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        before.head
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        before.source
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        before.registry
    );
    assert_eq!(snapshot_tree(fixture.target.path()), before.target);
    assert!(
        !fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .exists(),
        "rejected gate created a convergence transaction"
    );
}

#[test]
fn post_local_axes_are_reported_safe_to_apply() {
    let fixture = projected_fixture();
    for args in [
        vec!["--push-remote"],
        vec!["--require-runtime"],
        vec!["--push-remote", "--require-runtime"],
    ] {
        let (output, plan) = plan_converge(&fixture, &args);
        assert!(output.status.success(), "plan failed: {plan}");
        assert_eq!(
            plan["data"]["safe_to_apply"],
            json!(true),
            "post-local plan was not reported applyable: {plan}"
        );
    }
}

#[cfg(windows)]
#[test]
fn unsupported_atomic_refresh_and_source_exchange_are_non_executable() {
    let fixture = projected_fixture();
    let (output, refresh) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "refresh plan failed: {refresh}");
    assert_eq!(refresh["data"]["execution_enabled"], json!(false));
    assert_eq!(refresh["data"]["safe_to_apply"], json!(false));
    assert!(
        conflict_codes(&refresh).contains(&"PLATFORM_ATOMIC_PROJECTION_ACTIVATION_UNSUPPORTED")
    );

    let instance = refresh["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance");
    let (output, projection_input) =
        plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(
        output.status.success(),
        "projection-input plan failed: {projection_input}"
    );
    assert_eq!(projection_input["data"]["execution_enabled"], json!(false));
    assert_eq!(projection_input["data"]["safe_to_apply"], json!(false));
    assert!(
        conflict_codes(&projection_input).contains(&"PLATFORM_ATOMIC_SOURCE_EXCHANGE_UNSUPPORTED")
    );
}

#[test]
fn ineligible_projection_routes_are_sealed_as_conflicts() {
    for (ownership, disable_copy, expected) in [
        (Some("external"), false, "TARGET_NOT_MANAGED"),
        (None, true, "PROJECTION_METHOD_UNSUPPORTED"),
    ] {
        let fixture = projected_fixture();
        let path = fixture.root.path().join("state/registry/targets.json");
        let mut targets: Value =
            serde_json::from_slice(&fs::read(&path).expect("targets")).expect("parse targets");
        if let Some(ownership) = ownership {
            targets["targets"][0]["ownership"] = json!(ownership);
        }
        if disable_copy {
            targets["targets"][0]["capabilities"]["copy"] = json!(false);
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(&targets).expect("encode targets"),
        )
        .expect("write targets");
        git(fixture.root.path(), &["add", "state/registry/targets.json"]);
        git(
            fixture.root.path(),
            &["commit", "-m", "test: make route ineligible"],
        );

        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(
            output.status.success(),
            "ineligible plan was not reviewable: {plan}"
        );
        assert_eq!(plan["data"]["safe_to_apply"], json!(false));
        assert!(conflict_codes(&plan).contains(&expected));
        assert!(!plan["data"]["effects"].as_array().unwrap().is_empty());
        let (output, rejected) = apply_plan(&fixture, &plan, expected, &[]);
        assert!(
            !output.status.success(),
            "ineligible plan applied: {rejected}"
        );
        assert!(
            !fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json")
                .exists()
        );
    }
}

#[test]
fn uncommitted_checkpoint_is_not_an_apply_boundary() {
    let fixture = projected_fixture();
    let checkpoint_path = fixture
        .root
        .path()
        .join("state/registry/ops/checkpoint.json");
    let mut checkpoint: Value =
        serde_json::from_slice(&fs::read(&checkpoint_path).expect("checkpoint"))
            .expect("parse checkpoint");
    checkpoint["updated_at"] = json!("2000-01-01T00:00:00Z");
    fs::write(
        &checkpoint_path,
        serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint"),
    )
    .expect("write uncommitted checkpoint");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, rejected) = apply_plan(&fixture, &plan, "dirty-checkpoint", &[]);
    assert!(
        !output.status.success(),
        "uncommitted checkpoint was accepted: {rejected}"
    );
    assert_eq!(
        rejected["error"]["details"]["conflict"]["code"],
        json!("PLAN_CHECKPOINT_DRIFT")
    );
}

#[test]
fn stale_plan_and_lock_contention() {
    let stale_fixture = projected_fixture();
    let (output, stale_plan) = plan_converge(&stale_fixture, &[]);
    assert!(output.status.success(), "plan failed: {stale_plan}");
    fs::write(
        stale_fixture.root.path().join("skills/demo/details.txt"),
        "source drift\n",
    )
    .expect("mutate source");
    let (output, stale) = apply_plan(&stale_fixture, &stale_plan, "stale", &[]);
    assert!(!output.status.success(), "stale plan applied: {stale}");
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_SOURCE_DRIFT")
    );

    for lock_name in ["workspace.lock", "skill-demo.lock"] {
        let locked_fixture = projected_fixture();
        let (output, locked_plan) = plan_converge(&locked_fixture, &[]);
        assert!(output.status.success(), "plan failed: {locked_plan}");
        let locks = locked_fixture.root.path().join("state/locks");
        fs::create_dir_all(&locks).expect("create locks");
        fs::write(
            locks.join(lock_name),
            format!(
                "{{\"pid\":{},\"owner_id\":\"held\",\"host\":\"other-host\",\"created_at\":\"{}\"}}\n",
                std::process::id(),
                chrono::Utc::now().to_rfc3339()
            ),
        )
        .expect("hold convergence lock");
        let (output, busy) = apply_plan(&locked_fixture, &locked_plan, lock_name, &[]);
        assert!(
            !output.status.success(),
            "held {lock_name} bypassed: {busy}"
        );
        assert!(
            busy["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("LOCK_BUSY"))
        );
    }

    let head_fixture = projected_fixture();
    let (output, head_plan) = plan_converge(&head_fixture, &[]);
    assert!(output.status.success(), "plan failed: {head_plan}");
    git(
        head_fixture.root.path(),
        &["commit", "--allow-empty", "-m", "advance unrelated HEAD"],
    );
    let changed_head = git(head_fixture.root.path(), &["rev-parse", "HEAD"]);
    let (output, stale) = apply_plan(&head_fixture, &head_plan, "head-drift", &[]);
    assert!(!output.status.success(), "HEAD drift applied: {stale}");
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_STALE")
    );
    assert_eq!(
        git(head_fixture.root.path(), &["rev-parse", "HEAD"]),
        changed_head
    );

    let checkpoint_fixture = projected_fixture();
    let (output, checkpoint_plan) = plan_converge(&checkpoint_fixture, &[]);
    assert!(output.status.success(), "plan failed: {checkpoint_plan}");
    let checkpoint_path = checkpoint_fixture
        .root
        .path()
        .join("state/registry/ops/checkpoint.json");
    let mut checkpoint: Value =
        serde_json::from_slice(&fs::read(&checkpoint_path).expect("read registry checkpoint"))
            .expect("parse registry checkpoint");
    checkpoint["updated_at"] = json!("2000-01-01T00:00:00Z");
    let changed_checkpoint = serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint");
    fs::write(&checkpoint_path, &changed_checkpoint).expect("mutate registry checkpoint");
    let (output, stale) = apply_plan(
        &checkpoint_fixture,
        &checkpoint_plan,
        "checkpoint-drift",
        &[],
    );
    assert!(
        !output.status.success(),
        "checkpoint drift applied: {stale}"
    );
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_CHECKPOINT_DRIFT")
    );
    assert_eq!(
        fs::read(&checkpoint_path).expect("checkpoint after apply"),
        changed_checkpoint
    );

    let projection_fixture = projected_fixture();
    let (output, projection_plan) = plan_converge(&projection_fixture, &[]);
    assert!(output.status.success(), "plan failed: {projection_plan}");
    let live_marker = projection_fixture.target.path().join("demo/external.txt");
    fs::write(&live_marker, "external\n").expect("mutate live projection");
    let (output, stale) = apply_plan(
        &projection_fixture,
        &projection_plan,
        "projection-drift",
        &[],
    );
    assert!(
        !output.status.success(),
        "projection drift applied: {stale}"
    );
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_PROJECTION_DRIFT")
    );
    assert_eq!(
        fs::read_to_string(live_marker).expect("live marker"),
        "external\n"
    );
}

#[cfg(unix)]
#[test]
fn missing_symlink_refresh_is_stale_before_writes() {
    let fixture = projected_fixture_with_method("symlink");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("refresh"));
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let projection = fixture.target.path().join("demo");
    fs::remove_file(&projection).expect("remove planned symlink refresh");

    let (output, stale) = apply_plan(&fixture, &plan, "missing-symlink", &[]);
    assert!(!output.status.success(), "missing symlink applied: {stale}");
    assert_eq!(
        stale["error"]["details"]["conflict"]["code"],
        json!("PLAN_PROJECTION_DRIFT")
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        source
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        registry
    );
    assert!(!projection.exists());
    assert!(
        !fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .exists()
    );
}

#[test]
fn reviewed_missing_refresh_is_recreated() {
    let fixture = projected_fixture();
    fs::remove_dir_all(fixture.target.path().join("demo")).expect("remove live projection");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "missing refresh plan failed: {plan}"
    );
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("create"));
    assert_eq!(
        plan["data"]["input"]["projections"][0]["state"],
        json!("missing")
    );
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));

    let (output, applied) = apply_plan(&fixture, &plan, "missing-refresh", &[]);
    assert!(
        output.status.success(),
        "missing refresh apply failed: {applied}"
    );
    assert!(fixture.target.path().join("demo/SKILL.md").is_file());
}

#[test]
fn routing_drift_is_stale_before_writes() {
    for kind in ["rule-removed", "binding-inactive"] {
        let fixture = projected_fixture();
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let source = snapshot_tree(&fixture.root.path().join("skills/demo"));
        let target = snapshot_tree(fixture.target.path());
        let relative = match kind {
            "rule-removed" => "state/registry/rules.json",
            "binding-inactive" => "state/registry/bindings.json",
            _ => unreachable!(),
        };
        let path = fixture.root.path().join(relative);
        let mut registry: Value =
            serde_json::from_slice(&fs::read(&path).expect("read routing state"))
                .expect("parse routing state");
        match kind {
            "rule-removed" => registry["rules"] = json!([]),
            "binding-inactive" => registry["bindings"][0]["active"] = json!(false),
            _ => unreachable!(),
        }
        fs::write(
            &path,
            serde_json::to_vec_pretty(&registry).expect("encode routing state"),
        )
        .expect("write routing drift");
        let changed = fs::read(&path).expect("capture routing drift");

        let (output, stale) = apply_plan(&fixture, &plan, kind, &[]);
        assert!(!output.status.success(), "{kind} applied: {stale}");
        assert_eq!(
            stale["error"]["details"]["conflict"]["code"],
            json!("PLAN_CHECKPOINT_DRIFT")
        );
        assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(
            snapshot_tree(&fixture.root.path().join("skills/demo")),
            source
        );
        assert_eq!(snapshot_tree(fixture.target.path()), target);
        assert_eq!(fs::read(&path).expect("routing after rejection"), changed);
        assert!(
            !fixture
                .root
                .path()
                .join("state/transactions/convergence-demo.json")
                .exists()
        );
    }
}

#[test]
fn projection_input_rejects_ignored_only_canonical_source_bytes() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join(".gitignore"),
        "skills/demo/private.txt\n",
    )
    .expect("gitignore");
    git(fixture.root.path(), &["add", ".gitignore"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: ignore canonical source evidence"],
    );
    fs::write(
        fixture.root.path().join("skills/demo/private.txt"),
        "ignored canonical bytes\n",
    )
    .expect("ignored source bytes");
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("projection instance");

    let (output, blocked) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "reviewable plan failed: {blocked}");
    assert_eq!(blocked["data"]["safe_to_apply"], json!(false));
    assert!(conflict_codes(&blocked).contains(&"STALE_PROJECTION_INPUT"));
    assert!(
        blocked["data"]["input"]["source_dirty_paths"]
            .as_array()
            .is_some_and(|paths| paths.contains(&json!("skills/demo/private.txt")))
    );
}

#[cfg(unix)]
#[test]
fn gates_do_not_degrade_or_expand() {
    use std::os::unix::fs::symlink;

    let ownership = projected_fixture();
    let targets_path = ownership.root.path().join("state/registry/targets.json");
    let mut targets: Value =
        serde_json::from_slice(&fs::read(&targets_path).expect("read ownership targets"))
            .expect("parse ownership targets");
    targets["targets"][0]["ownership"] = json!("external");
    fs::write(
        &targets_path,
        serde_json::to_vec_pretty(&targets).expect("encode ownership targets"),
    )
    .expect("write ownership target");
    git(
        ownership.root.path(),
        &["add", "state/registry/targets.json"],
    );
    git(
        ownership.root.path(),
        &["commit", "-m", "test: make target externally owned"],
    );
    let (output, ownership_plan) = plan_converge(&ownership, &[]);
    assert!(
        output.status.success(),
        "ownership plan failed: {ownership_plan}"
    );
    let digest = reseal_plan_event(&ownership, &ownership_plan, clear_review_blockers);
    let before = mutation_snapshot(&ownership);
    let (output, blocked) =
        apply_resealed_plan(&ownership, &ownership_plan, &digest, "ownership-gate");
    assert!(
        !output.status.success(),
        "external ownership applied: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("PLAN_OWNERSHIP_DRIFT")
    );
    assert!(blocked["error"]["details"]["effect"].is_object());
    assert_rejected_before_writes(&ownership, before);

    let method = projected_fixture();
    let targets_path = method.root.path().join("state/registry/targets.json");
    let mut targets: Value =
        serde_json::from_slice(&fs::read(&targets_path).expect("read method targets"))
            .expect("parse method targets");
    targets["targets"][0]["capabilities"]["copy"] = json!(false);
    fs::write(
        &targets_path,
        serde_json::to_vec_pretty(&targets).expect("encode method targets"),
    )
    .expect("write method target");
    git(method.root.path(), &["add", "state/registry/targets.json"]);
    git(
        method.root.path(),
        &["commit", "-m", "test: remove reviewed method capability"],
    );
    let (output, method_plan) = plan_converge(&method, &[]);
    assert!(output.status.success(), "method plan failed: {method_plan}");
    let digest = reseal_plan_event(&method, &method_plan, clear_review_blockers);
    let before = mutation_snapshot(&method);
    let (output, blocked) = apply_resealed_plan(&method, &method_plan, &digest, "method-gate");
    assert!(
        !output.status.success(),
        "unsupported method applied: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("PLAN_METHOD_DRIFT")
    );
    assert_rejected_before_writes(&method, before);

    let policy = projected_fixture();
    let (output, policy_plan) = plan_converge(&policy, &[]);
    assert!(output.status.success(), "policy plan failed: {policy_plan}");
    let digest = reseal_plan_event(&policy, &policy_plan, |stored| {
        stored["preflight"]["checks"]["policy_safe_capture_digest"] = json!("sha256:degraded");
    });
    let before = mutation_snapshot(&policy);
    let (output, blocked) = apply_resealed_plan(&policy, &policy_plan, &digest, "policy-gate");
    assert!(
        !output.status.success(),
        "degraded policy evidence applied: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("PLAN_POLICY_DRIFT")
    );
    assert_rejected_before_writes(&policy, before);

    let approval = projected_fixture();
    write_skill(
        approval.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing approval drift.\ncapabilities:\n  shell:\n    commands: [\"git\"]\n---\n# demo\n",
    );
    let (output, saved) = save_skill(approval.root.path(), "demo");
    assert!(
        output.status.success(),
        "approval source save failed: {saved}"
    );
    let bindings: Value = serde_json::from_slice(
        &fs::read(approval.root.path().join("state/registry/bindings.json"))
            .expect("read approval bindings"),
    )
    .expect("parse approval bindings");
    let binding_id = bindings["bindings"][0]["binding_id"]
        .as_str()
        .expect("approval binding id");
    let (output, projected) = skill_project(approval.root.path(), "demo", binding_id, Some("copy"));
    assert!(
        output.status.success(),
        "approval refresh failed: {projected}"
    );
    let (output, approval_plan) = plan_converge(&approval, &[]);
    assert!(
        output.status.success(),
        "approval plan failed: {approval_plan}"
    );
    let digest = reseal_plan_event(&approval, &approval_plan, |stored| {
        clear_review_blockers(stored);
        stored["required_approvals"] = json!([]);
    });
    let before = mutation_snapshot(&approval);
    let (output, blocked) =
        apply_resealed_plan(&approval, &approval_plan, &digest, "approval-gate");
    assert!(
        !output.status.success(),
        "approval requirement removed: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("PLAN_APPROVAL_DRIFT")
    );
    assert_rejected_before_writes(&approval, before);

    let selectors = projected_fixture();
    add_copy_projection(&selectors, "selector-expansion");
    let (output, selector_plan) = plan_converge(&selectors, &[]);
    assert!(
        output.status.success(),
        "selector plan failed: {selector_plan}"
    );
    assert_eq!(
        selector_plan["data"]["projections"]
            .as_array()
            .expect("sealed projections")
            .len(),
        2
    );
    let digest = reseal_plan_event(&selectors, &selector_plan, |stored| {
        stored["projections"]
            .as_array_mut()
            .expect("projections")
            .pop();
        stored["visibility"]
            .as_array_mut()
            .expect("visibility")
            .pop();
    });
    let before = mutation_snapshot(&selectors);
    let (output, blocked) =
        apply_resealed_plan(&selectors, &selector_plan, &digest, "selector-gate");
    assert!(
        !output.status.success(),
        "selector scope expanded: {blocked}"
    );
    assert_eq!(
        blocked["error"]["details"]["conflict"]["code"],
        json!("PLAN_SELECTOR_SCOPE_DRIFT")
    );
    assert_rejected_before_writes(&selectors, before);

    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let reviewed_target = snapshot_tree(fixture.target.path());
    let reviewed_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let reviewed_source = snapshot_tree(&fixture.root.path().join("skills/demo"));
    let reviewed_registry = snapshot_tree(&fixture.root.path().join("state/registry"));
    let redirected = TestDir::new("convergence-redirected-target");
    for (relative, bytes) in &reviewed_target {
        let path = redirected.path().join(relative);
        fs::create_dir_all(path.parent().expect("redirected file parent"))
            .expect("create redirected parent");
        fs::write(path, bytes).expect("copy reviewed target bytes");
    }

    fs::remove_dir_all(fixture.target.path()).expect("remove reviewed target root");
    symlink(redirected.path(), fixture.target.path()).expect("redirect target root");
    let (output, rejected) = apply_plan(&fixture, &plan, "filesystem-scope-drift", &[]);
    assert_eq!(snapshot_tree(redirected.path()), reviewed_target);
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        reviewed_head
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("skills/demo")),
        reviewed_source
    );
    assert_eq!(
        snapshot_tree(&fixture.root.path().join("state/registry")),
        reviewed_registry
    );
    assert!(
        !fixture
            .root
            .path()
            .join("state/transactions/convergence-demo.json")
            .exists()
    );
    fs::remove_file(fixture.target.path()).expect("remove redirected target root");
    fs::create_dir_all(fixture.target.path()).expect("restore target root");
    for (relative, bytes) in reviewed_target {
        let path = fixture.target.path().join(relative);
        fs::create_dir_all(path.parent().expect("restored file parent"))
            .expect("create restored parent");
        fs::write(path, bytes).expect("restore reviewed target bytes");
    }

    assert!(
        !output.status.success(),
        "filesystem scope redirection applied outside the reviewed target: {rejected}"
    );
    assert_eq!(
        rejected["error"]["details"]["conflict"]["code"],
        json!("PLAN_FILESYSTEM_SCOPE_DRIFT")
    );
}
