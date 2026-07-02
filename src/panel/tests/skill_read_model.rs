use super::*;
use crate::panel::handlers::v1_skills;
use crate::state_model::{
    REGISTRY_SCHEMA_VERSION, RegistryProjectionTarget, RegistryTargetCapabilities,
};
use serde_json::json;

fn write_observed_targets(root: &Path, targets: Vec<RegistryProjectionTarget>) {
    let paths = RegistryStatePaths::from_root(root);
    paths
        .save_targets(&RegistryTargetsFile {
            schema_version: REGISTRY_SCHEMA_VERSION,
            targets,
        })
        .expect("save targets");
}

fn observed_target(target_id: &str, path: &Path) -> RegistryProjectionTarget {
    RegistryProjectionTarget {
        target_id: target_id.to_string(),
        agent: "claude".into(),
        path: path.display().to_string(),
        ownership: crate::core::vocab::Ownership::Observed,
        capabilities: RegistryTargetCapabilities {
            symlink: false,
            copy: true,
            watch: true,
        },
        created_at: Some(Utc::now()),
    }
}

#[tokio::test]
async fn v1_skills_derives_observed_targets_from_current_inventory_without_operations() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let observed = root.join("observed-target");
    let skill = observed.join("present-skill");
    fs::create_dir_all(&skill).expect("create observed skill");
    fs::write(skill.join("SKILL.md"), "# present\n").expect("write observed entrypoint");
    fs::create_dir_all(root.join("skills/present-skill")).expect("create source skill");
    fs::write(root.join("skills/present-skill/SKILL.md"), "# present\n")
        .expect("write source entrypoint");
    write_observed_targets(&root, vec![observed_target("target-observed", &observed)]);

    let (status, Json(payload)) = v1_skills(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    let skills = payload["data"]["skills"].as_array().expect("skills array");
    let present = skills
        .iter()
        .find(|item| item["skill_id"] == json!("present-skill"))
        .expect("present skill row");
    assert_eq!(present["observed_imported"], json!(true));
    assert_eq!(present["observed_target_ids"], json!(["target-observed"]));

    cleanup_root(root);
}

#[tokio::test]
async fn v1_skills_derives_unchanged_monitor_inventory_from_symlinked_observed_target() {
    let (root, state) = make_test_state();
    write_registry_snapshot(&root, REGISTRY_SCHEMA_VERSION);
    let source = root.join("external-source");
    let observed = root.join("observed-target");
    let source_skill = source.join("linked-skill");
    fs::create_dir_all(&source_skill).expect("create source skill");
    fs::write(source_skill.join("skill.md"), "# linked\n").expect("write source entrypoint");
    fs::create_dir_all(&observed).expect("create observed target");
    let link = observed.join("linked-skill");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_skill, &link).expect("symlink observed skill");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_skill, &link).expect("symlink observed skill");
    write_observed_targets(&root, vec![observed_target("target-observed", &observed)]);

    let (status, Json(payload)) = v1_skills(State(state)).await;

    assert_eq!(status, StatusCode::OK);
    let skills = payload["data"]["skills"].as_array().expect("skills array");
    let linked = skills
        .iter()
        .find(|item| item["skill_id"] == json!("linked-skill"))
        .expect("linked skill row");
    assert_eq!(linked["source_status"], json!("missing"));
    assert_eq!(linked["observed_imported"], json!(true));
    assert_eq!(linked["observed_target_ids"], json!(["target-observed"]));

    cleanup_root(root);
}
