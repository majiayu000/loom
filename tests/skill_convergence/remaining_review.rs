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
