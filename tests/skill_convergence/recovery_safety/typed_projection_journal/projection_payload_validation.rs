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

#[cfg(unix)]
#[test]
fn noop_source_committed_future_timestamps_are_zero_mutation() {
    let fixture = projected_fixture_with_method("symlink");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "future timestamps\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "noop-future-timestamps";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_source_commit"),
    );
    assert!(
        !output.status.success(),
        "source boundary passed: {interrupted}"
    );
    let (journal_path, mut journal) = transaction_journal(&fixture);
    assert_eq!(journal["phase"], "source_committed");
    assert_eq!(journal["projections"][0]["state"], "noop_prepared");
    let original_journal = fs::read(&journal_path).expect("original journal");
    let future = json!("2999-01-01T00:00:00Z");
    journal["projections"][0]["projection"]["last_observed_at"] = future.clone();
    journal["projections"][0]["projection"]["updated_at"] = future;
    write_journal(&journal_path, &journal);
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "future timestamps resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);

    fs::write(&journal_path, original_journal).expect("restore journal");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "timestamp repair failed: {recovered}"
    );
}

#[cfg(unix)]
#[test]
fn noop_reobservation_gap_retries_with_a_fresh_timestamp() {
    let fixture = projected_fixture_with_method("symlink");
    fs::write(
        fixture.root.path().join("skills/demo/details.txt"),
        "reobservation gap\n",
    )
    .expect("edit source");
    let (output, plan) = plan_converge(&fixture, &[]);
    assert!(output.status.success(), "plan failed: {plan}");
    let key = "noop-reobservation-gap";
    let (output, interrupted) = apply(
        &fixture,
        &plan,
        key,
        Some("convergence_interrupt_after_projection_reobservation"),
    );
    assert!(
        !output.status.success(),
        "reobservation gap passed: {interrupted}"
    );
    let (journal_path, journal) = transaction_journal(&fixture);
    assert_eq!(journal["phase"], "installing_projections");
    let prepared_timestamp = journal["projections"][0]["projection"]["updated_at"]
        .as_str()
        .expect("prepared timestamp")
        .to_string();

    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(output.status.success(), "gap recovery failed: {recovered}");
    let projections: Value = serde_json::from_slice(
        &fs::read(fixture.root.path().join("state/registry/projections.json"))
            .expect("projections registry"),
    )
    .expect("parse projections registry");
    let final_timestamp = projections["projections"][0]["updated_at"]
        .as_str()
        .expect("final timestamp");
    assert_ne!(final_timestamp, prepared_timestamp);
    assert!(!journal_path.exists());
}

#[cfg(unix)]
#[test]
fn sealed_plan_controlled_live_ancestor_alias_is_zero_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = projected_fixture();
    let (second_live, _) = add_copy_projection(&fixture, "alias-second");
    let second_live = fs::canonicalize(second_live).expect("canonical second live");
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
    let (journal_path, journal) = transaction_journal(&fixture);
    assert_eq!(
        journal["projections"]
            .as_array()
            .expect("projections")
            .len(),
        2
    );
    let second_index = journal["projections"]
        .as_array()
        .expect("projections")
        .iter()
        .position(|projection| {
            projection["materialized_path"].as_str()
                == Some(second_live.to_str().expect("second live string"))
        })
        .expect("second projection index");
    let first_index = usize::from(second_index == 0);
    let first_staging = PathBuf::from(
        journal["projections"][first_index]["staging_path"]
            .as_str()
            .expect("first staging"),
    );
    let second_root = second_live.parent().expect("second target root");
    let held_root = second_root.with_file_name("alias-second-held");
    fs::rename(second_root, &held_root).expect("hold second target root");
    let held_snapshot = snapshot_tree(&held_root);
    symlink(&first_staging, second_root).expect("alias second target into first staging");
    assert_eq!(
        journal["projections"][second_index]["materialized_path"]
            .as_str()
            .expect("journal second live"),
        second_live.to_str().expect("second live string")
    );
    let before = recovery_surface_snapshot(&fixture, &journal_path, &journal);

    let (output, rejected) = apply(&fixture, &plan, key, None);
    assert!(
        !output.status.success(),
        "cross projection alias resumed: {rejected}"
    );
    assert_recovery_surfaces_unchanged(&fixture, &journal_path, &journal, &before);
    assert_eq!(
        fs::read_link(second_root).expect("alias preserved"),
        first_staging
    );
    assert_eq!(snapshot_tree(&held_root), held_snapshot);

    fs::remove_file(second_root).expect("remove alias");
    fs::rename(&held_root, second_root).expect("restore second target root");
    let (output, recovered) = apply(&fixture, &plan, key, None);
    assert!(
        output.status.success(),
        "cross projection repair failed: {recovered}"
    );
}
