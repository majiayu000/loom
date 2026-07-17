use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

use super::inventory::{INVENTORY_PATH, parse_surface_inventory};
use super::surface_check::{command_variants, extract_loom_commands, join_continuation_lines};
use super::{
    ContractVersion, ExampleClassification, InventoryError, PublicArgv, parse_contract_version,
    validate_public_argv,
};

const SKILL_METADATA: &str = "skills/loom-registry/loom.skill.toml";
const HISTORY: &str = "docs/cli-contract-history.toml";
const CONTRACT_SOURCE: &str = "src/cli_contract.rs";

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
    let current_version =
        declared_contract_version(&read_current(repo_root, CONTRACT_SOURCE)?, CONTRACT_SOURCE)?;
    let current_version_text = format!(
        "{}.{}.{}",
        current_version.major, current_version.minor, current_version.patch
    );
    if !current_records.iter().any(|(version, range, note)| {
        version == &current_version_text && range == &current_range && !note.trim().is_empty()
    }) {
        return Err(InventoryError::new(format!(
            "CLI contract {current_version_text} and Skill range '{current_range}' require a current history record with migration note"
        )));
    }
    if let Some(base_inventory_raw) = git_show(repo_root, base, INVENTORY_PATH)? {
        let base_source = git_show(repo_root, base, CONTRACT_SOURCE)?.ok_or_else(|| {
            InventoryError::new(format!(
                "{base}:{CONTRACT_SOURCE}: missing CLI contract version source"
            ))
        })?;
        let base_version =
            declared_contract_version(&base_source, &format!("{base}:{CONTRACT_SOURCE}"))?;
        let current_inventory_raw = read_current(repo_root, INVENTORY_PATH)?;
        let compare_agent_capabilities = inventory_has_agent_capabilities(&base_inventory_raw)?;
        let base_capabilities =
            capability_set(&base_inventory_raw, compare_agent_capabilities, |path| {
                git_show(repo_root, base, path)?.ok_or_else(|| {
                    InventoryError::new(format!("{base}:{path}: inventoried surface is missing"))
                })
            })?;
        let current_capabilities =
            capability_set(&current_inventory_raw, compare_agent_capabilities, |path| {
                read_current(repo_root, path)
            })?;
        if compare_agent_capabilities {
            enforce_capability_version(
                base_version,
                current_version,
                &base_capabilities,
                &current_capabilities,
            )?;
        }
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

fn declared_contract_version(
    source: &str,
    location: &str,
) -> Result<ContractVersion, InventoryError> {
    let prefix = "pub const CLI_CONTRACT_VERSION: &str = \"";
    let value = source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .and_then(|rest| rest.strip_suffix("\";"))
        .ok_or_else(|| InventoryError::new(format!("{location}: missing CLI_CONTRACT_VERSION")))?;
    parse_contract_version(value)
        .map_err(|error| InventoryError::new(format!("{location}: {error}")))
}

fn capability_set<F>(
    inventory_raw: &str,
    include_agent_capabilities: bool,
    mut read_surface: F,
) -> Result<BTreeSet<String>, InventoryError>
where
    F: FnMut(&str) -> Result<String, InventoryError>,
{
    let inventory = parse_surface_inventory(inventory_raw, INVENTORY_PATH)?;
    let surfaces = inventory
        .surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface.path.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut capabilities = BTreeSet::new();
    for example in inventory.examples.iter().filter(|example| {
        matches!(
            example.classification,
            ExampleClassification::Executable | ExampleClassification::CommandReference
        )
    }) {
        let path = surfaces.get(example.surface.as_str()).ok_or_else(|| {
            InventoryError::new(format!(
                "{INVENTORY_PATH}: example '{}' references missing surface '{}'",
                example.id, example.surface
            ))
        })?;
        let source = read_surface(path)?;
        let lines = source.lines().collect::<Vec<_>>();
        if example.end_line > lines.len() {
            return Err(InventoryError::new(format!(
                "{path}: example '{}' exceeds surface length",
                example.id
            )));
        }
        for offset in example.start_line - 1..example.end_line {
            let logical_line = join_continuation_lines(&lines, offset, lines[offset]);
            for command in extract_loom_commands(&logical_line) {
                for argv in command_variants(&command, example.classification) {
                    let parsed = validate_public_argv(&argv).map_err(|error| {
                        InventoryError::new(format!(
                            "{path}: example '{}' has invalid public command {argv:?}: {}",
                            example.id, error.message
                        ))
                    })?;
                    capabilities.insert(public_command_capability(parsed));
                }
            }
        }
    }
    if include_agent_capabilities {
        capabilities.extend(
            inventory
                .agent_capabilities
                .iter()
                .map(|capability| format!("agent:{capability}")),
        );
    }
    for emitter in &inventory.next_action_emitters {
        capabilities.insert(format!("emitter:{}:{:?}", emitter.id, emitter.shape));
    }
    for mutation in &inventory.panel_mutations {
        capabilities.insert(format!(
            "panel:{}:{}:{}:{:?}:{}",
            mutation.action_id,
            mutation.backend_route,
            mutation.handler,
            mutation.binding,
            mutation.cli_argv.join("\u{1f}")
        ));
    }
    if capabilities.is_empty() {
        return Err(InventoryError::new("CLI contract capability set is empty"));
    }
    Ok(capabilities)
}

fn public_command_capability(parsed: PublicArgv) -> String {
    let mut explicit_args = parsed.explicit_args;
    explicit_args.retain(|argument| argument != "help");
    explicit_args.sort();
    explicit_args.dedup();
    format!(
        "command:{}:args={}",
        parsed.command_path.join("/"),
        explicit_args.join(",")
    )
}

fn inventory_has_agent_capabilities(raw: &str) -> Result<bool, InventoryError> {
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{INVENTORY_PATH}: {error}")))?;
    Ok(document
        .get("agent_capabilities")
        .and_then(toml_edit::Item::as_array)
        .is_some_and(|values| !values.is_empty()))
}

fn enforce_capability_version(
    base_version: ContractVersion,
    current_version: ContractVersion,
    base: &BTreeSet<String>,
    current: &BTreeSet<String>,
) -> Result<(), InventoryError> {
    if current_version < base_version {
        return Err(InventoryError::new(
            "CLI contract version must not move backwards",
        ));
    }
    let removed = base.difference(current).next().is_some();
    let added = current.difference(base).next().is_some();
    if removed && current_version.major <= base_version.major {
        return Err(InventoryError::new(
            "removed or changed CLI capabilities require a contract major bump",
        ));
    }
    if !removed
        && added
        && current_version.major == base_version.major
        && current_version.minor <= base_version.minor
    {
        return Err(InventoryError::new(
            "additive CLI capabilities require a contract minor bump",
        ));
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        ContractVersion, enforce_capability_version, public_command_capability,
        validate_public_argv,
    };

    fn version(major: u64, minor: u64, patch: u64) -> ContractVersion {
        ContractVersion {
            major,
            minor,
            patch,
        }
    }

    #[test]
    fn contract_additive_field_requires_minor_bump() {
        let base = BTreeSet::from(["agent:field:envelope.ok:boolean".to_string()]);
        let current = BTreeSet::from([
            "agent:field:envelope.ok:boolean".to_string(),
            "agent:field:envelope.request_id:string".to_string(),
        ]);
        let error = enforce_capability_version(version(1, 0, 0), version(1, 0, 1), &base, &current)
            .expect_err("patch bump must not admit an additive field");
        assert!(error.to_string().contains("minor bump"), "{error}");
        enforce_capability_version(version(1, 0, 0), version(1, 1, 0), &base, &current)
            .expect("minor bump admits an additive field");
    }

    #[test]
    fn contract_breaking_field_semantics_require_major_bump() {
        let base = BTreeSet::from(["agent:field:envelope.ok:boolean".to_string()]);
        let current = BTreeSet::from(["agent:field:envelope.ok:truthy-string".to_string()]);
        let error = enforce_capability_version(version(1, 0, 0), version(1, 1, 0), &base, &current)
            .expect_err("minor bump must not admit changed field semantics");
        assert!(error.to_string().contains("major bump"), "{error}");
        enforce_capability_version(version(1, 0, 0), version(2, 0, 0), &base, &current)
            .expect("major bump admits changed field semantics");
    }

    #[test]
    fn command_capability_ignores_fixture_values() {
        let first = validate_public_argv(["loom", "skill", "inspect", "alpha"])
            .expect("first public command");
        let second = validate_public_argv(["loom", "skill", "inspect", "beta"])
            .expect("second public command");
        assert_eq!(
            public_command_capability(first),
            public_command_capability(second)
        );
    }
}
