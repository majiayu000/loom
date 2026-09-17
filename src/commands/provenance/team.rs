use super::*;

pub(crate) fn planned_record_files(
    ctx: &AppContext,
    record: SkillSourceRecord,
    sources_snapshot: Option<&str>,
    lock_snapshot: Option<&str>,
) -> Result<(String, String)> {
    let mut sources: SkillSourcesFile = match sources_snapshot {
        Some(raw) => serde_json::from_str(raw)?,
        None => SkillSourcesFile {
            schema_version: 1,
            sources: Vec::new(),
        },
    };
    let mut lock: LoomLockFile = match lock_snapshot {
        Some(raw) => serde_json::from_str(raw)?,
        None => LoomLockFile {
            version: 1,
            skills: BTreeMap::new(),
        },
    };
    lock.skills.insert(
        record.skill_id.clone(),
        lock_skill_for_record(ctx, &record)?,
    );
    sources
        .sources
        .retain(|item| item.skill_id != record.skill_id);
    sources.sources.push(record);
    sources
        .sources
        .sort_by_cached_key(|entry| entry.skill_id.clone());
    Ok((
        serde_json::to_string_pretty(&sources)? + "\n",
        serde_json::to_string_pretty(&lock)? + "\n",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn planned_metadata_uses_the_captured_snapshot() {
        let root =
            std::env::temp_dir().join(format!("loom-team-snapshot-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("skills/new")).unwrap();
        fs::write(root.join("skills/new/SKILL.md"), "new").unwrap();
        let ctx = AppContext::new(Some(root.clone())).unwrap();
        let manifest = crate::commands::team_package::TeamArtifactManifest {
            service_origin: "https://team.example.test".into(),
            team_id: "t".into(),
            skill_id: "s".into(),
            version_id: "v".into(),
            sha256: "0".repeat(64),
            requested_ref: "v".into(),
        };
        let record =
            provenance_record_for_skill("new", manifest.descriptor(), &root.join("skills/new"))
                .unwrap();
        let mut other = record.clone();
        other.skill_id = "other".into();
        let original = serde_json::to_string(&SkillSourcesFile {
            schema_version: 1,
            sources: vec![other],
        })
        .unwrap();
        fs::create_dir_all(root.join("state/registry")).unwrap();
        fs::write(
            root.join(SOURCES_REL),
            "{\"schema_version\":1,\"sources\":[]}",
        )
        .unwrap();
        let (sources, lock) = planned_record_files(&ctx, record, Some(&original), None).unwrap();
        let parsed: SkillSourcesFile = serde_json::from_str(&sources).unwrap();
        assert_eq!(
            parsed
                .sources
                .iter()
                .map(|r| r.skill_id.as_str())
                .collect::<Vec<_>>(),
            ["new", "other"]
        );
        let input = crate::commands::team_package::TeamInput {
            manifest,
            input_path: String::new(),
            old_sources: Some(original),
            old_lock: None,
            new_sources: sources,
            new_lock: lock,
        };
        assert!(
            input.validate_metadata(&root, false).is_err(),
            "concurrent metadata edits must invalidate apply"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
