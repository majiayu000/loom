use std::fs;

use serde_json::json;

use super::*;
use crate::skill_convergence_executor::apply_plan;

#[cfg(unix)]
fn install_reference_transaction_hook(fixture: &Fixture, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    let hook = fixture.root.path().join(".git/hooks/reference-transaction");
    fs::write(&hook, script).expect("write reference transaction hook");
    let mut permissions = fs::metadata(&hook).expect("hook metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&hook, permissions).expect("make reference transaction hook executable");
}

#[cfg(unix)]
#[test]
fn materialize_symlink_digest_survives_registry_recovery() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture_with_method("materialize");
    let source = fixture.root.path().join("skills/demo");
    fs::write(source.join("real.txt"), "materialized bytes\n").expect("real file");
    symlink("real.txt", source.join("alias.txt")).expect("contained source symlink");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "materialize plan failed: {plan}");
    assert_ne!(
        plan["data"]["effects"][0]["source_tree_digest"],
        plan["data"]["input"]["selected_input_tree_digest"]
    );
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "materialize-recovery",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_committing_registry",
        )],
    );
    assert!(
        !output.status.success(),
        "materialize did not stop: {interrupted}"
    );
    let (output, recovered) = apply_plan(&fixture, &plan, "materialize-recovery", &[]);
    assert!(
        output.status.success(),
        "materialize recovery failed: {recovered}"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/alias.txt"))
            .expect("materialized alias"),
        "materialized bytes\n"
    );
}

#[test]
fn recovery_rechecks_routing_before_projection_writes() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/recovery.txt"),
        "new projection bytes\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "routing-recovery",
        &[(
            "LOOM_FAULT_INJECT",
            "convergence_interrupt_after_source_commit",
        )],
    );
    assert!(
        !output.status.success(),
        "source boundary did not stop: {interrupted}"
    );
    let target_before = snapshot_tree(fixture.target.path());
    let rules_path = fixture.root.path().join("state/registry/rules.json");
    let mut rules: Value =
        serde_json::from_slice(&fs::read(&rules_path).expect("rules")).expect("parse rules");
    rules["rules"] = json!([]);
    fs::write(
        &rules_path,
        serde_json::to_vec_pretty(&rules).expect("encode rules"),
    )
    .expect("remove live rule");

    let (output, rejected) = apply_plan(&fixture, &plan, "routing-recovery", &[]);
    assert!(
        !output.status.success(),
        "routing drift resumed: {rejected}"
    );
    assert_eq!(snapshot_tree(fixture.target.path()), target_before);
}

#[test]
fn ignored_only_source_bytes_are_committed_and_projected() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join(".gitignore"),
        "skills/demo/ignored.txt\n",
    )
    .expect("gitignore");
    git(fixture.root.path(), &["add", ".gitignore"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: ignore convergence fixture"],
    );
    fs::write(
        fixture.root.path().join("skills/demo/ignored.txt"),
        "ignored but selected\n",
    )
    .expect("ignored skill file");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(
        output.status.success(),
        "ignored source plan failed: {plan}"
    );
    let (output, applied) = apply_plan(&fixture, &plan, "ignored-source", &[]);
    assert!(
        output.status.success(),
        "ignored source apply failed: {applied}"
    );
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    assert_eq!(
        git(
            fixture.root.path(),
            &["show", &format!("{}:skills/demo/ignored.txt", head.trim())],
        ),
        "ignored but selected\n"
    );
    assert_eq!(
        fs::read_to_string(fixture.target.path().join("demo/ignored.txt"))
            .expect("projected ignored file"),
        "ignored but selected\n"
    );
}

#[cfg(unix)]
#[test]
fn source_changed_by_the_source_cas_hook_fails_closed() {
    let fixture = Fixture {
        root: TestDir::new("convergence-post-cas-source"),
        workspace: TestDir::new("convergence-post-cas-workspace"),
        target: TestDir::new("convergence-post-cas-target"),
    };
    write_skill(
        fixture.root.path(),
        "demo",
        "---\nname: demo\ndescription: Use when testing post-CAS source drift.\n---\n# demo\n",
    );
    git(fixture.root.path(), &["init"]);
    git(
        fixture.root.path(),
        &["config", "user.email", "tests@example.com"],
    );
    git(fixture.root.path(), &["config", "user.name", "Loom Tests"]);
    git(fixture.root.path(), &["add", "skills/demo"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: add post-CAS source"],
    );
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "reviewed source bytes\n",
    )
    .expect("source edit");
    let (output, plan) = run_loom(fixture.root.path(), &["plan", "converge", "demo"]);
    assert!(output.status.success(), "plan failed: {plan}");
    install_reference_transaction_hook(
        &fixture,
        "#!/bin/sh\nif [ \"$1\" = committed ]; then\n  printf 'external after source CAS\\n' > skills/demo/post-cas.txt\nfi\n",
    );

    let (output, rejected) = apply_plan(&fixture, &plan, "source-post-cas", &[]);
    assert!(
        !output.status.success(),
        "post-CAS source drift was accepted: {rejected}"
    );
    assert!(
        rejected["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("source")),
        "source drift failed for an unrelated reason: {rejected}"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("skills/demo/post-cas.txt"))
            .expect("external source bytes"),
        "external after source CAS\n"
    );
}

#[cfg(unix)]
#[test]
fn registry_changed_by_the_registry_cas_hook_fails_closed() {
    let fixture = projected_fixture();
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry post-CAS source\n",
    )
    .expect("source edit");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut external: Value = serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
        .expect("parse registry");
    external["projections"] = json!([]);
    let external_bytes = serde_json::to_vec_pretty(&external).expect("encode external registry");
    fs::write(
        fixture.root.path().join("state/external-projections.json"),
        &external_bytes,
    )
    .expect("external registry evidence");
    install_reference_transaction_hook(
        &fixture,
        "#!/bin/sh\nif [ \"$1\" = committed ] && git diff-tree --name-only -r HEAD | /usr/bin/grep -qx 'state/registry/projections.json'; then\n  /bin/cp state/external-projections.json state/registry/projections.json\nfi\n",
    );

    let (output, rejected) = apply_plan(&fixture, &plan, "registry-post-cas", &[]);
    assert!(
        !output.status.success(),
        "post-CAS registry drift was accepted: {rejected}"
    );
    assert_eq!(
        fs::read(&registry_path).expect("live external registry"),
        external_bytes
    );
}

#[test]
fn untracked_registry_routing_is_not_an_apply_boundary() {
    let fixture = projected_fixture();
    let relative = "state/registry/rules.json";
    let live = fs::read(fixture.root.path().join(relative)).expect("routing bytes");
    git(fixture.root.path(), &["rm", "--cached", relative]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: remove committed routing evidence"],
    );
    assert_eq!(
        fs::read(fixture.root.path().join(relative)).expect("untracked routing bytes"),
        live
    );
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let head = git(fixture.root.path(), &["rev-parse", "HEAD"]);

    let (output, rejected) = apply_plan(&fixture, &plan, "untracked-routing", &[]);
    assert!(
        !output.status.success(),
        "untracked routing was accepted: {rejected}"
    );
    assert_eq!(git(fixture.root.path(), &["rev-parse", "HEAD"]), head);
}

#[cfg(unix)]
#[test]
fn safe_symlink_refresh_preparation_does_not_stage_beside_target() {
    let fixture = projected_fixture_with_method("symlink");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let (output, interrupted) = apply_plan(
        &fixture,
        &plan,
        "safe-symlink-no-stage",
        &[("LOOM_FAULT_INJECT", "convergence_interrupt_after_prepared")],
    );
    assert!(
        !output.status.success(),
        "prepare fault passed: {interrupted}"
    );
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let journal: Value =
        serde_json::from_slice(&fs::read(journal_path).expect("journal")).expect("parse journal");
    let projection = &journal["projections"][0];
    let staging_owner = Path::new(projection["staging_owner"].as_str().expect("staging owner"));
    let staging_path = Path::new(projection["staging_path"].as_str().expect("staging path"));
    assert!(
        !staging_owner.exists() && !staging_path.exists(),
        "safe symlink no-op must not reserve writable target staging"
    );
    let attempt = journal["ownership_attempts"]
        .as_array()
        .expect("ownership attempts")
        .iter()
        .find(|attempt| attempt["destination"] == json!(staging_owner.display().to_string()))
        .expect("projection ownership attempt");
    assert_eq!(attempt["state"], json!("allocated"));
}

#[test]
fn projection_source_swap_excludes_nested_git_metadata() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance");
    let projection = fixture.target.path().join("demo");
    fs::create_dir_all(projection.join(".git/objects")).expect("nested git dir");
    fs::write(projection.join(".git/config"), "[core]\n").expect("nested git config");

    let (output, plan) = plan_converge(&fixture, &["--from-projection", "--instance", instance]);
    assert!(output.status.success(), "projection plan failed: {plan}");
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));
    let (output, applied) = apply_plan(&fixture, &plan, "nested-git", &[]);
    assert!(
        output.status.success(),
        "nested Git metadata broke apply: {applied}"
    );
    assert!(
        !fixture.root.path().join("skills/demo/.git").exists(),
        "nested Git metadata entered canonical source"
    );
}

#[test]
fn create_projection_materializes_a_missing_target_root() {
    let fixture = projected_fixture();
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
        .expect("parse registry");
    registry["projections"] = json!([]);
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("encode registry"),
    )
    .expect("clear registry projection");
    git(
        fixture.root.path(),
        &["add", "state/registry/projections.json"],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: remove projection record"],
    );
    fs::remove_dir_all(fixture.target.path()).expect("remove target root");

    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "create plan failed: {plan}");
    assert_eq!(plan["data"]["effects"][0]["effect"], json!("create"));
    let (output, applied) = apply_plan(&fixture, &plan, "missing-target-root", &[]);
    assert!(
        output.status.success(),
        "missing target root blocked apply: {applied}"
    );
    assert!(fixture.target.path().join("demo/SKILL.md").is_file());
}

#[test]
fn projection_candidate_is_validated_instead_of_blocked_canonical_source() {
    let fixture = projected_fixture();
    let (output, initial) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "initial plan failed: {initial}");
    let instance = initial["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("instance")
        .to_string();

    let scripts = fixture.root.path().join("skills/demo/scripts");
    fs::create_dir_all(&scripts).expect("scripts dir");
    fs::write(scripts.join("danger.sh"), "#!/bin/sh\nrm -rf /\n").expect("unsafe source");
    git(fixture.root.path(), &["add", "skills/demo"]);
    git(
        fixture.root.path(),
        &["commit", "-m", "test: blocked canonical source"],
    );
    let source_revision = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
        .expect("parse registry");
    registry["projections"][0]["last_applied_rev"] = json!(source_revision.trim());
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("encode registry"),
    )
    .expect("update projection baseline");
    let bindings_path = fixture.root.path().join("state/registry/bindings.json");
    let mut bindings: Value = serde_json::from_slice(&fs::read(&bindings_path).expect("bindings"))
        .expect("parse bindings");
    bindings["bindings"][0]["policy_profile"] = json!("deny-risky");
    fs::write(
        &bindings_path,
        serde_json::to_vec_pretty(&bindings).expect("encode bindings"),
    )
    .expect("require strict activation policy");
    git(
        fixture.root.path(),
        &[
            "add",
            "state/registry/projections.json",
            "state/registry/bindings.json",
        ],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: record safe projection baseline"],
    );

    let (output, plan) = plan_converge(
        &fixture,
        &["--from-projection", "--instance", instance.as_str()],
    );
    assert!(output.status.success(), "projection plan failed: {plan}");
    assert_eq!(plan["data"]["safe_to_apply"], json!(true));
    let (output, applied) = apply_plan(&fixture, &plan, "candidate-safety", &[]);
    assert!(
        output.status.success(),
        "canonical source blocked reviewed candidate: {applied}"
    );
    assert!(
        !fixture
            .root
            .path()
            .join("skills/demo/scripts/danger.sh")
            .exists()
    );
}

#[cfg(unix)]
#[test]
fn projection_input_symlink_stage_targets_final_canonical_source() {
    let fixture = projected_fixture();
    let selected_plan = plan_converge(&fixture, &[]).1;
    let selected = selected_plan["data"]["effects"][0]["instance_id"]
        .as_str()
        .expect("selected instance")
        .to_string();
    let symlink_target = fixture.root.path().join("live/symlink-target");
    let (output, target) = target_add(fixture.root.path(), "claude", &symlink_target, "managed");
    assert!(output.status.success(), "target add failed: {target}");
    let target_id = target["data"]["target"]["target_id"]
        .as_str()
        .expect("target id");
    let workspace = fixture.workspace.path().to_str().expect("workspace");
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
    let (output, projected) =
        skill_project(fixture.root.path(), "demo", binding_id, Some("symlink"));
    assert!(
        output.status.success(),
        "symlink project failed: {projected}"
    );
    let symlink_instance = projected["data"]["projection"]["instance_id"]
        .as_str()
        .expect("symlink instance")
        .to_string();
    fs::remove_file(symlink_target.join("demo")).expect("remove symlink projection");
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let mut registry: Value = serde_json::from_slice(&fs::read(&registry_path).expect("registry"))
        .expect("parse registry");
    registry["projections"]
        .as_array_mut()
        .expect("projections")
        .retain(|projection| projection["instance_id"] != json!(symlink_instance));
    fs::write(
        &registry_path,
        serde_json::to_vec_pretty(&registry).expect("encode registry"),
    )
    .expect("remove symlink record");
    git(
        fixture.root.path(),
        &["add", "state/registry/projections.json"],
    );
    git(
        fixture.root.path(),
        &["commit", "-m", "test: require symlink activation"],
    );
    fs::write(
        fixture.target.path().join("demo/details.txt"),
        "selected projection bytes\n",
    )
    .expect("edit selected projection");

    let (output, plan) = plan_converge(
        &fixture,
        &["--from-projection", "--instance", selected.as_str()],
    );
    assert!(output.status.success(), "projection plan failed: {plan}");
    let (output, applied) = apply_plan(&fixture, &plan, "symlink-final-source", &[]);
    assert!(
        output.status.success(),
        "symlink activation failed: {applied}"
    );
    assert_eq!(
        symlink_target
            .join("demo")
            .canonicalize()
            .expect("canonical symlink"),
        fixture
            .root
            .path()
            .join("skills/demo")
            .canonicalize()
            .expect("canonical source")
    );
}

#[cfg(unix)]
#[test]
fn noop_source_commit_retires_after_a_head_changed_during_index_preparation() {
    let fixture = projected_fixture();
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let original = plan["data"]["source"]["registry_head"]
        .as_str()
        .expect("planned head")
        .to_string();
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");
    let (output, rejected) = std::thread::scope(|scope| {
        let apply = scope.spawn(|| {
            apply_plan(
                &fixture,
                &plan,
                "noop-head-race",
                &[("LOOM_TEST_CONVERGENCE_NOOP_SOURCE_PAUSE_MS", "2000")],
            )
        });
        let mut entered_noop_source = false;
        for _ in 0..200 {
            if let Ok(raw) = fs::read(&journal_path)
                && let Ok(journal) = serde_json::from_slice::<Value>(&raw)
                && journal["phase"] == json!("committing_source")
                && !journal["source_staged_index_digest"].is_null()
            {
                entered_noop_source = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            entered_noop_source,
            "apply never entered no-op source boundary"
        );
        fs::write(fixture.root.path().join("external-head.txt"), "external\n")
            .expect("external file");
        git(fixture.root.path(), &["add", "external-head.txt"]);
        git(
            fixture.root.path(),
            &["commit", "-m", "test: concurrent no-op source head"],
        );
        apply.join().expect("apply thread")
    });
    assert!(
        !output.status.success(),
        "external HEAD was accepted: {rejected}"
    );
    let external = git(fixture.root.path(), &["rev-parse", "HEAD"]);
    assert_ne!(external.trim(), original);
    assert!(
        !journal_path.exists(),
        "active no-op journal was not retired"
    );
    assert!(
        Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(fixture.root.path())
            .status()
            .expect("inspect external index")
            .success(),
        "transaction changed the external commit index"
    );
    let retained = fs::read_dir(journal_path.parent().expect("journal parent"))
        .expect("retained journal directory")
        .filter_map(Result::ok)
        .find(|entry| entry.file_name().to_string_lossy().starts_with("retained-"))
        .expect("retained no-op journal");
    let retained: Value =
        serde_json::from_slice(&fs::read(retained.path()).expect("retained journal"))
            .expect("parse retained journal");
    assert_eq!(retained["phase"], json!("rolled_back_artifacts_retained"));
    assert_eq!(retained["source_index_changed"], json!(false));
    assert!(retained["source_head"].is_null());

    let (output, fresh) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "fresh plan failed: {fresh}");
    let (output, applied) = apply_plan(&fixture, &fresh, "noop-head-race-fresh", &[]);
    assert!(
        output.status.success(),
        "retired no-op journal blocked a fresh apply: {applied}"
    );
}

#[cfg(unix)]
#[test]
fn external_head_between_registry_guard_and_json_cas_restores_owned_surfaces() {
    let fixture = projected_fixture();
    let registry_path = fixture.root.path().join("state/registry/projections.json");
    let original_registry = fs::read(&registry_path).expect("original registry projections");
    let live_projection = fixture.target.path().join("demo");
    let original_projection = snapshot_tree(&live_projection);
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "registry save race\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let journal_path = fixture
        .root
        .path()
        .join("state/transactions/convergence-demo.json");

    let (output, rejected, external_head) = std::thread::scope(|scope| {
        let apply = scope.spawn(|| {
            apply_plan(
                &fixture,
                &plan,
                "registry-save-head-race",
                &[("LOOM_TEST_CONVERGENCE_REGISTRY_SAVE_PAUSE_MS", "2000")],
            )
        });
        let mut entered_registry_save = false;
        for _ in 0..200 {
            if let Ok(raw) = fs::read(&journal_path)
                && let Ok(journal) = serde_json::from_slice::<Value>(&raw)
                && journal["phase"] == json!("projections_swapped")
                && snapshot_tree(&live_projection) != original_projection
            {
                entered_registry_save = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(entered_registry_save, "apply never entered registry save");
        fs::write(fixture.root.path().join("external-head.txt"), "external\n")
            .expect("external file");
        git(fixture.root.path(), &["add", "external-head.txt"]);
        git(
            fixture.root.path(),
            &["commit", "-m", "test: concurrent registry save head"],
        );
        let external_head = git(fixture.root.path(), &["rev-parse", "HEAD"]);
        let (output, rejected) = apply.join().expect("apply thread");
        (output, rejected, external_head)
    });
    assert!(
        !output.status.success(),
        "external HEAD was accepted without a fresh plan: {rejected}"
    );
    assert_eq!(
        git(fixture.root.path(), &["rev-parse", "HEAD"]),
        external_head
    );
    assert_eq!(
        fs::read(&registry_path).expect("restored registry projections"),
        original_registry
    );
    assert_eq!(snapshot_tree(&live_projection), original_projection);
    assert!(
        !journal_path.exists(),
        "active registry race journal remained"
    );
    assert!(
        Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(fixture.root.path())
            .status()
            .expect("inspect external index")
            .success(),
        "transaction changed the external commit index"
    );

    let (output, fresh) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "fresh plan failed: {fresh}");
    let (output, applied) = apply_plan(&fixture, &fresh, "registry-save-head-fresh", &[]);
    assert!(
        output.status.success(),
        "restored registry race blocked a fresh apply: {applied}"
    );
}
