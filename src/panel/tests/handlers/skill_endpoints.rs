use super::*;

#[tokio::test]
async fn v1_skills_returns_union_read_model() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let paths = RegistryStatePaths::from_root(&root);
    let source_dir = root.join("skills/present-skill");
    fs::create_dir_all(&source_dir).expect("create present skill");
    fs::write(
        source_dir.join("SKILL.md"),
        "---\nname: present-skill\ndescription: \"Shows the panel skill description\"\n---\n# present\n",
    )
    .expect("write skill");
    fs::create_dir_all(root.join("skills/broken-skill")).expect("create broken skill");

    paths
        .save_rules(&RegistryRulesFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            rules: vec![
                RegistryBindingRule {
                    binding_id: "binding-1".to_string(),
                    skill_id: "present-skill".to_string(),
                    target_id: "target-1".to_string(),
                    method: crate::core::vocab::ProjectionMethod::Symlink,
                    watch_policy: "observe_only".to_string(),
                    created_at: Some(Utc::now()),
                },
                RegistryBindingRule {
                    binding_id: "binding-2".to_string(),
                    skill_id: "rule-only".to_string(),
                    target_id: "target-2".to_string(),
                    method: crate::core::vocab::ProjectionMethod::Copy,
                    watch_policy: "observe_only".to_string(),
                    created_at: Some(Utc::now()),
                },
            ],
        })
        .expect("save rules");
    paths
        .save_projections(&RegistryProjectionsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            projections: vec![RegistryProjectionInstance {
                instance_id: "inst-projected".to_string(),
                skill_id: "projected-only".to_string(),
                binding_id: None,
                target_id: "target-3".to_string(),
                materialized_path: "/tmp/projected".to_string(),
                method: crate::core::vocab::ProjectionMethod::Copy,
                last_applied_rev: "abcdef1234567890".to_string(),
                health: crate::core::vocab::Health::Healthy,
                observed_drift: Some(false),
                source_tree_digest: None,
                materialized_tree_digest: None,
                last_observed_at: None,
                last_observed_error: None,
                updated_at: Some(Utc::now()),
            }],
        })
        .expect("save projections");
    paths
        .append_operation(&RegistryOperationRecord {
            op_id: "op-observed".to_string(),
            intent: "skill.import_observed".to_string(),
            status: "succeeded".to_string(),
            ack: false,
            payload: json!({}),
            effects: json!({
                "imported": [{"skill": "observed-only", "target_id": "target-observed"}],
                "skipped": [
                    {"skill": "present-skill", "target_id": "target-1", "reason": "already-exists"},
                    {"skill": "ignored-invalid", "target_id": "target-1", "reason": "invalid-skill-name"}
                ]
            }),
            last_error: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        })
        .expect("append op");

    let (status, Json(payload)) = v1_skills(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("registry.skills"));
    assert_eq!(payload["error"], Value::Null);
    assert_eq!(payload["data"]["count"], json!(5));

    let skills = payload["data"]["skills"].as_array().expect("skills array");
    let by_id = |skill_id: &str| {
        skills
            .iter()
            .find(|item| item["skill_id"] == json!(skill_id))
            .unwrap_or_else(|| panic!("missing skill {skill_id}: {skills:?}"))
    };

    assert_eq!(by_id("present-skill")["source_status"], json!("present"));
    assert_eq!(
        by_id("present-skill")["description"],
        json!("Shows the panel skill description")
    );
    assert_eq!(by_id("present-skill")["bindings_count"], json!(1));
    assert_eq!(
        by_id("broken-skill")["source_status"],
        json!("non-compliant")
    );
    assert_eq!(by_id("rule-only")["source_status"], json!("missing"));
    assert_eq!(by_id("projected-only")["projections_count"], json!(1));
    assert_eq!(
        by_id("projected-only")["latest_rev"],
        json!("abcdef1234567890")
    );
    assert_eq!(by_id("observed-only")["observed_imported"], json!(true));
    assert_eq!(
        by_id("observed-only")["observed_target_ids"],
        json!(["target-observed"])
    );
    assert_eq!(
        by_id("present-skill")["observed_target_ids"],
        json!(["target-1"])
    );

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skills_warns_when_description_cannot_be_read() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let source_dir = root.join("skills/unreadable-description");
    fs::create_dir_all(&source_dir).expect("create skill");
    fs::write(
        source_dir.join("SKILL.md"),
        b"---\ndescription: \xFF\n---\n# present\n",
    )
    .expect("write invalid skill description");

    let (status, Json(payload)) = v1_skills(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["data"]["count"], json!(1));
    assert_eq!(
        payload["data"]["skills"][0]["skill_id"],
        json!("unreadable-description")
    );
    assert_eq!(
        payload["data"]["skills"][0]["source_status"],
        json!("present")
    );
    assert_eq!(payload["data"]["skills"][0]["description"], Value::Null);

    let warnings = payload["meta"]["warnings"]
        .as_array()
        .expect("warnings array");
    assert!(
        warnings.iter().any(|warning| warning
            .as_str()
            .is_some_and(|message| message.contains("failed to read skill description"))),
        "missing description warning: {warnings:?}"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skill_diagnose_returns_envelope_without_command_audit() {
    let (root, state) = make_test_state();
    let source_dir = root.join("skills/present-skill");
    fs::create_dir_all(&source_dir).expect("create skill");
    fs::write(
        source_dir.join("SKILL.md"),
        "---\ndescription: Present skill\n---\n",
    )
    .expect("write skill");

    let (status, Json(payload)) = v1_skill_diagnose(
        axum::extract::Path("present-skill".to_string()),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.diagnose"));
    assert_eq!(payload["data"]["skill"], json!("present-skill"));
    assert!(
        !root.join("state/events/commands.jsonl").exists(),
        "interactive panel diagnose must not append command-audit rows"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skill_inspect_returns_shared_read_model_without_command_audit() {
    let (root, state) = make_test_state();
    let source_dir = root.join("skills/present-skill");
    fs::create_dir_all(&source_dir).expect("create skill");
    fs::write(
        source_dir.join("SKILL.md"),
        "---\nname: present-skill\ndescription: Present skill\n---\n# Present\n",
    )
    .expect("write skill");

    let (status, Json(payload)) = v1_skill_inspect(
        axum::extract::Path("present-skill".to_string()),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.inspect"));
    assert_eq!(payload["data"]["skill"], json!("present-skill"));
    assert_eq!(payload["data"]["source"]["entrypoint"], json!("SKILL.md"));
    assert!(payload["data"]["runtime"].is_object());
    assert!(payload["data"]["next_actions"].is_array());
    assert!(
        !root.join("state/events/commands.jsonl").exists(),
        "interactive panel inspect must not append command-audit rows"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skill_inspect_missing_skill_returns_structured_error() {
    let (root, state) = make_test_state();

    let (status, Json(payload)) =
        v1_skill_inspect(axum::extract::Path("missing".to_string()), State(state)).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(payload["ok"], json!(false));
    assert_eq!(payload["cmd"], json!("skill.inspect"));
    assert_eq!(payload["error"]["code"], json!("SKILL_NOT_FOUND"));

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skill_trash_lists_entries_without_command_audit() {
    let (root, state) = make_test_state();
    let trash_id = "demo-20260604T010203Z-a1b2c3d4";
    let entry_dir = root.join("trash").join(trash_id);
    fs::create_dir_all(&entry_dir).expect("create trash entry");
    fs::write(
        entry_dir.join("metadata.json"),
        json!({
            "schema_version": 1,
            "trash_id": trash_id,
            "skill": "demo",
            "original_path": "skills/demo",
            "trashed_at": "2026-06-04T01:02:03Z",
            "source_commit": "abcdef1234567890"
        })
        .to_string(),
    )
    .expect("write metadata");

    let (status, Json(payload)) = v1_skill_trash(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.trash.list"));
    assert_eq!(payload["data"]["items"][0]["trash_id"], json!(trash_id));
    assert_eq!(payload["data"]["items"][0]["skill"], json!("demo"));
    assert!(
        !root.join("state/events/commands.jsonl").exists(),
        "interactive panel trash list must not append command-audit rows"
    );

    cleanup_root(root);
}

#[tokio::test]
async fn registry_skill_trash_add_and_restore_use_cli_envelopes() {
    let (root, state) = make_test_state();
    let skill_dir = root.join("skills/trash-demo");
    fs::create_dir_all(&skill_dir).expect("create skill");
    fs::write(skill_dir.join("SKILL.md"), "# Trash demo\n").expect("write skill");
    crate::gitops::ensure_repo_initialized(state.ctx.as_ref()).expect("init git");
    git_ok(&root, &["add", "skills/trash-demo"]);
    git_ok(&root, &["commit", "-m", "add trash demo"]);

    let (add_status, Json(add_payload)) = registry_skill_trash_add(
        axum::extract::Path("trash-demo".to_string()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state.clone()),
    )
    .await;

    assert_eq!(add_status, StatusCode::OK);
    assert_eq!(add_payload["ok"], json!(true));
    assert_eq!(add_payload["cmd"], json!("skill.trash.add"));
    let trash_id = add_payload["data"]["trash_id"]
        .as_str()
        .expect("trash id")
        .to_string();
    assert!(!root.join("skills/trash-demo").exists());
    assert!(root.join("trash").join(&trash_id).exists());

    let (restore_status, Json(restore_payload)) = registry_skill_trash_restore(
        axum::extract::Path(trash_id.clone()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
        Json(TrashRestoreRequest {
            skill: "trash-demo".to_string(),
        }),
    )
    .await;

    assert_eq!(restore_status, StatusCode::OK);
    assert_eq!(restore_payload["ok"], json!(true));
    assert_eq!(restore_payload["cmd"], json!("skill.trash.restore"));
    assert!(root.join("skills/trash-demo/SKILL.md").exists());
    assert!(!root.join("trash").join(trash_id).exists());

    cleanup_root(root);
}

#[tokio::test]
async fn registry_skill_trash_purge_removes_one_entry() {
    let (root, state) = make_test_state();
    let trash_id = "purge-demo-20260604T010203Z-a1b2c3d4";
    let entry_dir = root.join("trash").join(trash_id);
    fs::create_dir_all(entry_dir.join("skill")).expect("create trash entry");
    fs::write(entry_dir.join("skill/SKILL.md"), "# Purge demo\n").expect("write payload");
    fs::write(
        entry_dir.join("metadata.json"),
        json!({
            "schema_version": 1,
            "trash_id": trash_id,
            "skill": "purge-demo",
            "original_path": "skills/purge-demo",
            "trashed_at": "2026-06-04T01:02:03Z",
            "source_commit": "abcdef1234567890"
        })
        .to_string(),
    )
    .expect("write metadata");
    crate::gitops::ensure_repo_initialized(state.ctx.as_ref()).expect("init git");
    git_ok(&root, &["add", "trash"]);
    git_ok(&root, &["commit", "-m", "add trash entry"]);

    let (status, Json(payload)) = registry_skill_trash_purge(
        axum::extract::Path(trash_id.to_string()),
        ConnectInfo(panel_peer()),
        panel_headers(),
        State(state),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["ok"], json!(true));
    assert_eq!(payload["cmd"], json!("skill.trash.purge"));
    assert!(!entry_dir.exists());

    cleanup_root(root);
}
