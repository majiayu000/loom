use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::gitops;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};

use super::super::PanelState;

#[derive(Debug, Default)]
pub(super) struct SkillReadRow {
    pub(super) skill_id: String,
    pub(super) source_path: Option<PathBuf>,
    pub(super) source_status: Option<&'static str>,
    pub(super) sources: BTreeSet<&'static str>,
    pub(super) binding_ids: BTreeSet<String>,
    pub(super) target_ids: BTreeSet<String>,
    pub(super) projection_count: usize,
    pub(super) latest_rev: Option<String>,
    pub(super) latest_updated_at: Option<String>,
    pub(super) release_tags: Vec<String>,
    pub(super) snapshot_tags: Vec<String>,
    pub(super) observed_imported: bool,
}

impl SkillReadRow {
    pub(super) fn new(skill_id: String) -> Self {
        Self {
            skill_id,
            ..Self::default()
        }
    }
}

pub(super) fn build_skill_read_model(
    state: &PanelState,
) -> anyhow::Result<(Vec<serde_json::Value>, Vec<String>, bool)> {
    let mut warnings = Vec::new();
    let mut rows: BTreeMap<String, SkillReadRow> = BTreeMap::new();

    add_source_skill_rows(&state.ctx.skills_dir, &mut rows)?;

    let paths = RegistryStatePaths::from_app_context(&state.ctx);
    let snapshot = paths.maybe_load_snapshot()?;
    let registry_available = snapshot.is_some();
    if let Some(snapshot) = snapshot.as_ref() {
        add_registry_skill_rows(snapshot, &mut rows);
        add_observed_import_rows(snapshot, &mut rows);
    } else {
        warnings.push(format!(
            "registry state not initialized under {}",
            paths.registry_dir.display()
        ));
    }

    add_skill_tags(state, &mut rows, &mut warnings)?;

    Ok((
        rows.into_values()
            .map(skill_row_to_json)
            .collect::<Vec<_>>(),
        warnings,
        registry_available,
    ))
}

fn skill_row<'a>(
    rows: &'a mut BTreeMap<String, SkillReadRow>,
    skill_id: &str,
) -> &'a mut SkillReadRow {
    rows.entry(skill_id.to_string())
        .or_insert_with(|| SkillReadRow::new(skill_id.to_string()))
}

fn add_source_skill_rows(
    skills_dir: &Path,
    rows: &mut BTreeMap<String, SkillReadRow>,
) -> anyhow::Result<()> {
    if !skills_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(skills_dir)? {
        let entry = entry?;
        let skill_id = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let row = skill_row(rows, &skill_id);
        row.sources.insert("source");
        row.source_path = Some(path.clone());
        row.source_status = Some(if path.is_dir() && path.join("SKILL.md").is_file() {
            "present"
        } else {
            "non-compliant"
        });
    }
    Ok(())
}

fn add_registry_skill_rows(snapshot: &RegistrySnapshot, rows: &mut BTreeMap<String, SkillReadRow>) {
    for rule in &snapshot.rules.rules {
        let row = skill_row(rows, &rule.skill_id);
        row.sources.insert("rule");
        row.binding_ids.insert(rule.binding_id.clone());
        row.target_ids.insert(rule.target_id.clone());
    }

    for projection in &snapshot.projections.projections {
        let row = skill_row(rows, &projection.skill_id);
        row.sources.insert("projection");
        if let Some(binding_id) = projection.binding_id.as_ref() {
            row.binding_ids.insert(binding_id.clone());
        }
        row.target_ids.insert(projection.target_id.clone());
        row.projection_count += 1;
        if !projection.last_applied_rev.is_empty()
            && row.latest_rev.is_none()
            && projection.updated_at.is_none()
        {
            row.latest_rev = Some(projection.last_applied_rev.clone());
        }
        if let Some(updated_at) = projection.updated_at {
            let updated_at = updated_at.to_rfc3339();
            if row
                .latest_updated_at
                .as_ref()
                .is_none_or(|current| updated_at > *current)
            {
                row.latest_updated_at = Some(updated_at);
                row.latest_rev = Some(projection.last_applied_rev.clone());
            }
        }
    }
}

fn add_observed_import_rows(
    snapshot: &RegistrySnapshot,
    rows: &mut BTreeMap<String, SkillReadRow>,
) {
    for op in &snapshot.operations {
        if op.intent != "skill.import_observed" && op.intent != "skill.monitor_observed" {
            continue;
        }
        for field in ["imported", "updated"] {
            if let Some(items) = op.effects.get(field).and_then(serde_json::Value::as_array) {
                for item in items {
                    if let Some(skill_id) = item.get("skill").and_then(serde_json::Value::as_str) {
                        let row = skill_row(rows, skill_id);
                        row.sources.insert("observed");
                        row.observed_imported = true;
                    }
                }
            }
        }
    }
}

fn add_skill_tags(
    state: &PanelState,
    rows: &mut BTreeMap<String, SkillReadRow>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    if !gitops::repo_is_initialized(&state.ctx)? {
        warnings.push(
            "git repository not initialized; release and snapshot tags unavailable".to_string(),
        );
        return Ok(());
    }

    let output = gitops::run_git_allow_failure(&state.ctx, &["tag", "--list"])?;
    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "failed to list git tags: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for tag in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(rest) = tag.strip_prefix("release/") {
            if let Some((skill_id, version)) = rest.split_once('/') {
                let row = skill_row(rows, skill_id);
                row.sources.insert("release_tag");
                row.release_tags.push(version.to_string());
            }
        } else if let Some(rest) = tag.strip_prefix("snapshot/")
            && let Some((skill_id, snapshot)) = rest.split_once('/')
        {
            let row = skill_row(rows, skill_id);
            row.sources.insert("snapshot_tag");
            row.snapshot_tags.push(snapshot.to_string());
        }
    }
    Ok(())
}

pub(super) fn skill_row_to_json(row: SkillReadRow) -> serde_json::Value {
    let source_status = row.source_status.unwrap_or("missing");
    json!({
        "skill_id": row.skill_id,
        "source_status": source_status,
        "source_path": row.source_path.map(|path| path.display().to_string()),
        "latest_rev": row.latest_rev,
        "latest_updated_at": row.latest_updated_at,
        "bindings_count": row.binding_ids.len(),
        "projections_count": row.projection_count,
        "target_ids": row.target_ids.into_iter().collect::<Vec<_>>(),
        "release_tags": row.release_tags,
        "snapshot_tags": row.snapshot_tags,
        "observed_imported": row.observed_imported,
        "sources": row.sources.into_iter().collect::<Vec<_>>(),
    })
}
