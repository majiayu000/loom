use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

use super::InventoryError;

const SKILL_METADATA: &str = "skills/loom-registry/loom.skill.toml";
const HISTORY: &str = "docs/cli-contract-history.toml";

pub fn check_contract_range_policy(
    repo_root: &Path,
    diff_base: Option<&str>,
) -> Result<(), InventoryError> {
    let base = diff_base
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            InventoryError::new("LOOM_CONTRACT_DIFF_BASE is required and must not be empty")
        })?;
    let tree = format!("{base}^{{tree}}");
    let status = Command::new("git")
        .current_dir(repo_root)
        .args(["cat-file", "-e", &tree])
        .status()
        .map_err(|error| InventoryError::new(format!("git cat-file failed: {error}")))?;
    if !status.success() {
        return Err(InventoryError::new(format!(
            "contract diff base is not reachable: {base}"
        )));
    }
    let current_range = contract_range(&read_current(repo_root, SKILL_METADATA)?, SKILL_METADATA)?;
    let base_range = git_show(repo_root, base, SKILL_METADATA)?
        .map(|raw| contract_range_optional(&raw, SKILL_METADATA))
        .transpose()?
        .flatten();
    let current_records = history_records(&read_current(repo_root, HISTORY)?)?;
    if current_records.is_empty() {
        return Err(InventoryError::new(
            "CLI contract history must not be empty",
        ));
    }
    if let Some(base_history) = git_show(repo_root, base, HISTORY)?
        && !history_records(&base_history)?.is_subset(&current_records)
    {
        return Err(InventoryError::new("CLI contract history is append-only"));
    }
    if base_range.as_deref() != Some(current_range.as_str()) {
        if !current_records
            .iter()
            .any(|(_, range, note)| range == &current_range && !note.trim().is_empty())
        {
            return Err(InventoryError::new(format!(
                "Skill range '{current_range}' requires a migration note"
            )));
        }
        let changelog = read_current(repo_root, "CHANGELOG.md")?;
        if !changelog.contains("CLI compatibility") && !changelog.contains("CLI contract") {
            return Err(InventoryError::new(
                "Skill range changes require a CHANGELOG CLI contract note",
            ));
        }
    }
    Ok(())
}

fn contract_range(raw: &str, location: &str) -> Result<String, InventoryError> {
    contract_range_optional(raw, location)?.ok_or_else(|| {
        InventoryError::new(format!("{location}: missing compatibility.cli_contract"))
    })
}

fn contract_range_optional(raw: &str, location: &str) -> Result<Option<String>, InventoryError> {
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{location}: invalid TOML: {error}")))?;
    let value = document
        .get("compatibility")
        .and_then(|item| item.as_table())
        .and_then(|table| table.get("cli_contract"))
        .and_then(|item| item.as_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    Ok(value)
}

fn history_records(raw: &str) -> Result<BTreeSet<(String, String, String)>, InventoryError> {
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{HISTORY}: invalid TOML: {error}")))?;
    let tables = document
        .get("contract")
        .and_then(|item| item.as_array_of_tables())
        .ok_or_else(|| InventoryError::new(format!("{HISTORY}: missing [[contract]] records")))?;
    tables
        .iter()
        .map(|table| {
            let field = |name: &str| {
                table
                    .get(name)
                    .and_then(|item| item.as_str())
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .ok_or_else(|| InventoryError::new(format!("{HISTORY}: empty contract.{name}")))
            };
            Ok((
                field("version")?,
                field("skill_range")?,
                field("migration_note")?,
            ))
        })
        .collect()
}

fn git_show(repo_root: &Path, base: &str, path: &str) -> Result<Option<String>, InventoryError> {
    let listing = Command::new("git")
        .current_dir(repo_root)
        .args(["ls-tree", "-r", "--name-only", base, "--", path])
        .output()
        .map_err(|error| InventoryError::new(format!("git ls-tree failed: {error}")))?;
    if !listing.status.success() {
        return Err(InventoryError::new(format!(
            "git ls-tree failed for {base}:{path}: {}",
            String::from_utf8_lossy(&listing.stderr).trim()
        )));
    }
    if listing.stdout.is_empty() {
        return Ok(None);
    }
    let spec = format!("{base}:{path}");
    let output = Command::new("git")
        .current_dir(repo_root)
        .args(["show", &spec])
        .output()
        .map_err(|error| InventoryError::new(format!("git show failed: {error}")))?;
    if !output.status.success() {
        return Err(InventoryError::new(format!(
            "git show failed for {spec}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    String::from_utf8(output.stdout)
        .map(Some)
        .map_err(|error| InventoryError::new(format!("{spec}: non-UTF-8 content: {error}")))
}

fn read_current(repo_root: &Path, path: &str) -> Result<String, InventoryError> {
    fs::read_to_string(repo_root.join(path))
        .map_err(|error| InventoryError::new(format!("{path}: {error}")))
}
