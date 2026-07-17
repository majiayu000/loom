use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use toml_edit::DocumentMut;

use super::inventory::{INVENTORY_PATH, parse_surface_inventory};
use super::{ContractVersion, InventoryError, contract_version_matches, parse_contract_version};

const SKILL_METADATA: &str = "skills/loom-registry/loom.skill.toml";
const HISTORY: &str = "docs/cli-contract-history.toml";
const CONTRACT_SOURCE: &str = "src/cli_contract.rs";
const COMMAND_TREE_SNAPSHOT_VERSION: u64 = 1;

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
    ensure_contract_range_contains_version(&current_range, &current_version_text)?;
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
        let base_capabilities = capability_set(&base_inventory_raw, compare_agent_capabilities)?;
        let current_capabilities =
            capability_set(&current_inventory_raw, compare_agent_capabilities)?;
        if compare_agent_capabilities {
            enforce_capability_transition(
                base_version,
                current_version,
                &base_capabilities,
                &current_capabilities,
                command_tree_snapshot_version(
                    &base_inventory_raw,
                    &format!("{base}:{INVENTORY_PATH}"),
                )?,
                command_tree_snapshot_version(&current_inventory_raw, INVENTORY_PATH)?,
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
        if !changelog_diff_has_contract_note(repo_root, base)? {
            return Err(InventoryError::new(
                "Skill range changes require a CHANGELOG CLI contract note",
            ));
        }
    }
    Ok(())
}

fn changelog_diff_has_contract_note(repo_root: &Path, base: &str) -> Result<bool, InventoryError> {
    let output = Command::new("git")
        .current_dir(repo_root)
        .args([
            "diff",
            "--no-ext-diff",
            "--unified=0",
            base,
            "--",
            "CHANGELOG.md",
        ])
        .output()
        .map_err(|error| InventoryError::new(format!("git diff failed: {error}")))?;
    if !output.status.success() {
        return Err(InventoryError::new(format!(
            "git diff failed for CHANGELOG.md: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let diff = String::from_utf8(output.stdout)
        .map_err(|error| InventoryError::new(format!("git diff returned non-UTF-8: {error}")))?;
    Ok(diff.lines().any(|line| {
        line.starts_with('+')
            && !line.starts_with("+++")
            && (line.contains("CLI compatibility") || line.contains("CLI contract"))
    }))
}

pub(super) fn ensure_contract_range_contains_version(
    contract_range: &str,
    contract_version: &str,
) -> Result<(), InventoryError> {
    let matches = contract_version_matches(contract_range, contract_version).map_err(|error| {
        InventoryError::new(format!(
            "{SKILL_METADATA}: invalid compatibility.cli_contract: {error}"
        ))
    })?;
    if !matches {
        return Err(InventoryError::new(format!(
            "Skill CLI contract range '{contract_range}' does not contain current CLI contract version '{contract_version}'"
        )));
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

fn capability_set(
    inventory_raw: &str,
    include_agent_capabilities: bool,
) -> Result<BTreeSet<String>, InventoryError> {
    let inventory = parse_surface_inventory(inventory_raw, INVENTORY_PATH)?;
    let mut capabilities = BTreeSet::new();
    if include_agent_capabilities {
        if inventory.command_capabilities.is_empty() {
            return Err(InventoryError::new(
                "command capability snapshot must not be empty",
            ));
        }
        capabilities.extend(
            inventory
                .agent_capabilities
                .iter()
                .map(|capability| format!("agent:{capability}")),
        );
        capabilities.extend(
            inventory
                .command_capabilities
                .iter()
                .map(|capability| format!("cli:{capability}")),
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

fn inventory_has_agent_capabilities(raw: &str) -> Result<bool, InventoryError> {
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{INVENTORY_PATH}: {error}")))?;
    Ok(document
        .get("agent_capabilities")
        .and_then(toml_edit::Item::as_array)
        .is_some_and(|values| !values.is_empty()))
}

fn command_tree_snapshot_version(raw: &str, location: &str) -> Result<Option<u64>, InventoryError> {
    let document = raw
        .parse::<DocumentMut>()
        .map_err(|error| InventoryError::new(format!("{location}: {error}")))?;
    let Some(item) = document.get("command_tree_snapshot_version") else {
        return Ok(None);
    };
    let value = item.as_integer().ok_or_else(|| {
        InventoryError::new(format!(
            "{location}: command_tree_snapshot_version must be an integer"
        ))
    })?;
    let value = u64::try_from(value).map_err(|_| {
        InventoryError::new(format!(
            "{location}: command_tree_snapshot_version must be positive"
        ))
    })?;
    if value != COMMAND_TREE_SNAPSHOT_VERSION {
        return Err(InventoryError::new(format!(
            "{location}: unsupported command_tree_snapshot_version {value}"
        )));
    }
    Ok(Some(value))
}

fn enforce_capability_transition(
    base_version: ContractVersion,
    current_version: ContractVersion,
    base: &BTreeSet<String>,
    current: &BTreeSet<String>,
    base_tree_snapshot: Option<u64>,
    current_tree_snapshot: Option<u64>,
) -> Result<(), InventoryError> {
    match (base_tree_snapshot, current_tree_snapshot) {
        (Some(_), None) => {
            return Err(InventoryError::new(
                "removing command_tree_snapshot_version requires a contract major bump",
            ));
        }
        (None, Some(COMMAND_TREE_SNAPSHOT_VERSION)) => {
            let removed = base.difference(current).next().is_some();
            let has_non_cli_addition = current
                .difference(base)
                .any(|capability| !capability.starts_with("cli:"));
            if current_version == base_version && !removed && !has_non_cli_addition {
                return Ok(());
            }
        }
        _ => {}
    }
    enforce_capability_version(base_version, current_version, base, current)
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
    let removed = base
        .difference(current)
        .any(|capability| !replaced_aggregate_fingerprint(capability, current));
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

fn replaced_aggregate_fingerprint(capability: &str, current: &BTreeSet<String>) -> bool {
    let kind = capability.strip_prefix("cli:").unwrap_or(capability);
    if !kind.starts_with("argument-group:") {
        return false;
    }
    let Some((identity, _)) = capability.rsplit_once(":sha256:") else {
        return false;
    };
    current.iter().any(|candidate| {
        candidate
            .rsplit_once(":sha256:")
            .is_some_and(|(candidate_identity, _)| candidate_identity == identity)
    })
}

pub(super) fn contract_range(raw: &str, location: &str) -> Result<String, InventoryError> {
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
        ContractVersion, enforce_capability_transition, enforce_capability_version,
        ensure_contract_range_contains_version,
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
    fn public_command_tree_capabilities_follow_contract_semver() {
        let base = BTreeSet::from([
            "cli:command:loom".to_string(),
            "cli:command:loom/status".to_string(),
        ]);
        let additive = BTreeSet::from([
            "cli:command:loom".to_string(),
            "cli:command:loom/doctor".to_string(),
            "cli:command:loom/status".to_string(),
        ]);
        let error =
            enforce_capability_version(version(1, 0, 0), version(1, 0, 1), &base, &additive)
                .expect_err("patch bump must not admit a new public command");
        assert!(error.to_string().contains("minor bump"), "{error}");
        enforce_capability_version(version(1, 0, 0), version(1, 1, 0), &base, &additive)
            .expect("minor bump admits a new public command");

        let removed = BTreeSet::from(["cli:command:loom".to_string()]);
        let error = enforce_capability_version(version(1, 0, 0), version(1, 1, 0), &base, &removed)
            .expect_err("minor bump must not admit removal of a public command");
        assert!(error.to_string().contains("major bump"), "{error}");
        enforce_capability_version(version(1, 0, 0), version(2, 0, 0), &base, &removed)
            .expect("major bump admits removal of a public command");
    }

    #[test]
    fn additive_argument_group_fingerprint_replacement_requires_only_minor_bump() {
        let base = BTreeSet::from([
            "argument-core:loom/apply:plan_id:sha256:stable".to_string(),
            "argument-group:loom/apply:ApplyArgs:sha256:old".to_string(),
        ]);
        let additive = BTreeSet::from([
            "argument-core:loom/apply:plan_digest:sha256:new".to_string(),
            "argument-core:loom/apply:plan_id:sha256:stable".to_string(),
            "argument-group:loom/apply:ApplyArgs:sha256:new".to_string(),
        ]);
        let error =
            enforce_capability_version(version(1, 1, 0), version(1, 1, 1), &base, &additive)
                .expect_err("patch bump must not admit an additive optional flag");
        assert!(error.to_string().contains("minor bump"), "{error}");
        enforce_capability_version(version(1, 1, 0), version(1, 2, 0), &base, &additive)
            .expect("minor bump admits an additive optional flag and replacement fingerprint");

        let removed_group =
            BTreeSet::from(["argument-core:loom/apply:plan_id:sha256:stable".to_string()]);
        let error =
            enforce_capability_version(version(1, 1, 0), version(1, 2, 0), &base, &removed_group)
                .expect_err("removing an aggregate group without replacement remains breaking");
        assert!(error.to_string().contains("major bump"), "{error}");
    }

    #[test]
    fn public_command_tree_snapshot_bootstrap_is_one_time_and_cli_only() {
        let base = BTreeSet::from(["cli:command:loom".to_string()]);
        let current = BTreeSet::from([
            "cli:command:loom".to_string(),
            "cli:command:loom/status".to_string(),
        ]);
        enforce_capability_transition(
            version(1, 0, 0),
            version(1, 0, 0),
            &base,
            &current,
            None,
            Some(1),
        )
        .expect("first explicit CLI tree snapshot is a bootstrap");

        let error = enforce_capability_transition(
            version(1, 1, 0),
            version(1, 0, 0),
            &base,
            &current,
            None,
            Some(1),
        )
        .expect_err("tree snapshot bootstrap must not permit version rollback");
        assert!(error.to_string().contains("backwards"), "{error}");

        let error = enforce_capability_transition(
            version(1, 0, 0),
            version(1, 0, 1),
            &base,
            &current,
            None,
            Some(1),
        )
        .expect_err("patch bump must not admit additive CLI capabilities");
        assert!(error.to_string().contains("minor bump"), "{error}");
        enforce_capability_transition(
            version(1, 0, 0),
            version(1, 1, 0),
            &base,
            &current,
            None,
            Some(1),
        )
        .expect("minor bump admits additive CLI capabilities during snapshot adoption");

        let mut non_cli = current.clone();
        non_cli.insert("agent:field:new".to_string());
        let error = enforce_capability_transition(
            version(1, 0, 0),
            version(1, 0, 0),
            &base,
            &non_cli,
            None,
            Some(1),
        )
        .expect_err("bootstrap must not hide non-CLI capability additions");
        assert!(error.to_string().contains("minor bump"), "{error}");

        let error = enforce_capability_transition(
            version(1, 0, 0),
            version(1, 0, 0),
            &base,
            &base,
            Some(1),
            None,
        )
        .expect_err("an established tree snapshot marker must not disappear");
        assert!(error.to_string().contains("snapshot_version"), "{error}");
    }

    #[test]
    fn shipped_skill_range_must_contain_current_contract_version() {
        ensure_contract_range_contains_version(">=1.0.0,<2.0.0", "1.5.0")
            .expect("compatible range");
        let error = ensure_contract_range_contains_version(">=1.0.0,<2.0.0", "2.0.0")
            .expect_err("incompatible shipped Skill range must fail");
        assert!(error.to_string().contains("does not contain"), "{error}");
    }
}
