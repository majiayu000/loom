use super::*;

#[test]
fn nested_prepared_observation_payload_tampering_is_zero_mutation() {
    for field in [
        "health",
        "source_tree_digest",
        "materialized_tree_digest",
        "observed_drift",
        "last_observed_at",
    ] {
        let fixture = projected_fixture();
        fs::write(
            fixture.root.path().join("skills/demo/details.txt"),
            format!("nested observation {field}\n"),
        )
        .expect("edit source");
        let (output, plan) = plan_converge(&fixture, &[]);
        assert!(output.status.success(), "plan failed: {plan}");
        let key = format!("nested-observation-{field}");
        let (output, interrupted) = apply(
            &fixture,
            &plan,
            &key,
            Some("convergence_interrupt_after_prepared"),
        );
        assert!(
            !output.status.success(),
            "prepared boundary passed: {interrupted}"
        );
        let (journal_path, mut journal) = transaction_journal(&fixture);
        let original_journal = fs::read(&journal_path).expect("original journal");
        let nested = &mut journal["projections"][0]["prepared"]["projection"];
        nested[field] = match field {
            "health" => json!("orphaned"),
            "source_tree_digest" | "materialized_tree_digest" => json!("tampered-digest"),
            "observed_drift" => json!(true),
            "last_observed_at" => Value::Null,
            _ => unreachable!(),
        };
        write_journal(&journal_path, &journal);
        let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

        let (output, rejected) = apply(&fixture, &plan, &key, None);
        assert!(
            !output.status.success(),
            "nested {field} tampering resumed: {rejected}"
        );
        assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

        fs::write(&journal_path, original_journal).expect("restore journal");
        let (output, recovered) = apply(&fixture, &plan, &key, None);
        assert!(
            output.status.success(),
            "nested {field} repair failed: {recovered}"
        );
    }
}

#[cfg(unix)]
#[test]
fn noop_prepared_outer_payload_tampering_is_zero_mutation() {
    let fixture = projected_fixture_with_method("symlink");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "noop payload\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "noop-outer-payload";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_prepared"),
    );
    assert!(
        !output.status.success(),
        "prepared boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    assert_eq!(journal["projections"][0]["state"], "noop_prepared");
    assert!(journal["projections"][0]["prepared"].is_null());
    let original_journal = fs::read(&journal_path).expect("original journal");
    journal["projections"][0]["projection"]["health"] = json!("orphaned");
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "noop payload tampering resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "noop payload repair failed: {recovered}"
    );
}

#[test]
fn cross_projection_claim_live_alias_is_zero_mutation() {
    let fixture = projected_fixture();
    add_copy_projection(&fixture, "alias-second");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "cross projection alias\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "cross-projection-alias";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_prepared"),
    );
    assert!(
        !output.status.success(),
        "prepared boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    assert_eq!(
        journal["projections"]
            .as_array()
            .expect("projections")
            .len(),
        2
    );
    let original_journal = fs::read(&journal_path).expect("original journal");
    let first_staging = PathBuf::from(
        journal["projections"][0]["staging_path"]
            .as_str()
            .expect("first staging"),
    );
    journal["projections"][1]["materialized_path"] = json!(
        claim_path(&first_staging, ".finalize-claim")
            .display()
            .to_string()
    );
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "cross projection alias resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "cross projection repair failed: {recovered}"
    );
}
