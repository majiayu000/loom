use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::state::{AppContext, resolve_agent_skill_source_dirs};

// ---------------------------------------------------------------------------
// SkillInventory
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SkillInventory {
    pub source_skills: Vec<String>,
    pub backup_skills: Vec<String>,
    pub source_dirs: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn collect_skill_inventory(ctx: &AppContext) -> SkillInventory {
    let source_dirs = resolve_agent_skill_source_dirs(&ctx.root);
    let mut warnings = Vec::new();

    let source_skills = list_unique_skills_from_dirs(&source_dirs, "source", &mut warnings);
    let backup_skills = list_unique_skills_from_dirs(
        std::slice::from_ref(&ctx.skills_dir),
        "backup",
        &mut warnings,
    );

    SkillInventory {
        source_skills,
        backup_skills,
        source_dirs,
        warnings,
    }
}

fn list_unique_skills_from_dirs(
    dirs: &[PathBuf],
    label: &str,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut skills = BTreeSet::new();

    for dir in dirs {
        if !dir.exists() {
            continue;
        }

        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) => {
                warnings.push(format!(
                    "failed to read {} skills dir {}: {}",
                    label,
                    dir.display(),
                    err
                ));
                continue;
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    warnings.push(format!(
                        "failed to read entry in {} skills dir {}: {}",
                        label,
                        dir.display(),
                        err
                    ));
                    continue;
                }
            };

            let is_dir = match entry.file_type() {
                Ok(kind) if kind.is_dir() => true,
                Ok(kind) if kind.is_symlink() => fs::metadata(entry.path())
                    .map(|meta| meta.is_dir())
                    .unwrap_or(false),
                Ok(_) => false,
                Err(err) => {
                    warnings.push(format!(
                        "failed to inspect entry {} in {} skills dir {}: {}",
                        entry.file_name().to_string_lossy(),
                        label,
                        dir.display(),
                        err
                    ));
                    false
                }
            };

            if is_dir {
                skills.insert(entry.file_name().to_string_lossy().to_string());
            }
        }
    }

    skills.into_iter().collect()
}
