use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use crate::cli::SkillProvenanceOutdatedArgs;
use crate::envelope::Meta;
use crate::fs_util::remove_path_if_exists;
use crate::state::AppContext;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::{
    SkillSourceRecord, SourceDescriptor, clone_git_source, join_checked_subdir,
    load_record_for_skill, load_sources, skill_tree_digest,
};

const STATUS_UP_TO_DATE: &str = "up_to_date";
const STATUS_OUTDATED: &str = "outdated";
const STATUS_UNREACHABLE: &str = "unreachable";
const STATUS_UNPINNED_CANDIDATE: &str = "unpinned_candidate";
const STATUS_INVALID_SOURCE: &str = "invalid_source";

#[derive(Debug, Clone, Serialize)]
struct ProviderOutdatedRow {
    skill_id: String,
    provider: String,
    status: &'static str,
    current_ref: Option<String>,
    current_digest: String,
    candidate_ref: Option<String>,
    candidate_digest: Option<String>,
    candidate_trust: &'static str,
    source_locator: String,
    risk: Vec<String>,
    next_actions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize)]
struct ProviderOutdatedSummary {
    up_to_date: usize,
    outdated: usize,
    unreachable: usize,
    unpinned_candidate: usize,
    invalid_source: usize,
}

impl ProviderOutdatedSummary {
    fn observe(&mut self, status: &str) {
        match status {
            STATUS_UP_TO_DATE => self.up_to_date += 1,
            STATUS_OUTDATED => self.outdated += 1,
            STATUS_UNREACHABLE => self.unreachable += 1,
            STATUS_UNPINNED_CANDIDATE => self.unpinned_candidate += 1,
            STATUS_INVALID_SOURCE => self.invalid_source += 1,
            _ => {}
        }
    }
}

pub(super) fn cmd_provenance_outdated(
    ctx: &AppContext,
    args: &SkillProvenanceOutdatedArgs,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    let records = if let Some(skill) = &args.skill {
        vec![load_record_for_skill(ctx, skill)?]
    } else {
        load_sources(ctx)
            .map_err(map_io)?
            .sources
            .into_iter()
            .filter(is_provider_backed_record)
            .collect()
    };

    let generated_at = Utc::now();
    let staging_root =
        std::env::temp_dir().join(format!("loom-provenance-outdated-{}", uuid::Uuid::new_v4()));
    let mut rows = Vec::new();
    for record in records {
        rows.push(provider_outdated_row(ctx, &record, &staging_root));
    }
    let _ = remove_path_if_exists(&staging_root);

    let mut summary = ProviderOutdatedSummary::default();
    for row in &rows {
        summary.observe(row.status);
    }
    let re_pin_plan = args.plan.then(|| re_pin_plan_for_rows(&rows, generated_at));
    Ok((
        json!({
            "generated_at": generated_at,
            "plan_requested": args.plan,
            "count": rows.len(),
            "summary": summary,
            "rows": rows,
            "re_pin_plan": re_pin_plan,
            "next_actions": [
                "review rows with status outdated or unpinned_candidate before applying any re-pin",
                "combine with `loom skill provenance verify <skill>` to confirm installed content still matches recorded provenance",
            ],
        }),
        Meta::default(),
    ))
}

fn is_provider_backed_record(record: &SkillSourceRecord) -> bool {
    !matches!(record.source.provider.as_str(), "git" | "local_path")
}

fn provider_outdated_row(
    ctx: &AppContext,
    record: &SkillSourceRecord,
    staging_root: &Path,
) -> ProviderOutdatedRow {
    if !is_provider_backed_record(record) {
        return status_row(
            record,
            STATUS_INVALID_SOURCE,
            None,
            None,
            "unavailable",
            vec!["source was not installed through a provider-backed import".to_string()],
            Some("provider outdated checks require provider-backed provenance".to_string()),
        );
    }
    if record.source.path.is_some() {
        return local_provider_outdated_row(record);
    }
    if record.source.repository.is_some() {
        return git_provider_outdated_row(ctx, record, staging_root);
    }
    status_row(
        record,
        STATUS_INVALID_SOURCE,
        None,
        None,
        "unavailable",
        vec!["source record has neither local path nor git repository metadata".to_string()],
        Some("provider source metadata is incomplete".to_string()),
    )
}

fn local_provider_outdated_row(record: &SkillSourceRecord) -> ProviderOutdatedRow {
    let Some(base_path) = record.source.path.as_deref() else {
        return status_row(
            record,
            STATUS_INVALID_SOURCE,
            None,
            None,
            "unavailable",
            vec!["local provider source is missing path metadata".to_string()],
            Some("local provider source path is missing".to_string()),
        );
    };
    let source_path = join_checked_subdir(Path::new(base_path), &record.source.subdir);
    if !source_path.is_dir() {
        return status_row(
            record,
            STATUS_UNREACHABLE,
            None,
            None,
            "unavailable",
            vec!["local provider source cannot be read".to_string()],
            Some(format!(
                "local provider source '{}' is not a directory",
                source_path.display()
            )),
        );
    }
    let candidate_digest = match skill_tree_digest(&source_path) {
        Ok(digest) => digest,
        Err(err) => {
            return status_row(
                record,
                STATUS_UNREACHABLE,
                None,
                None,
                "unavailable",
                vec!["local provider source digest could not be computed".to_string()],
                Some(err.to_string()),
            );
        }
    };
    let current_ref = current_ref_for_source(&record.source);
    if !current_ref.as_deref().is_some_and(is_sha256_ref) {
        return status_row(
            record,
            STATUS_UNPINNED_CANDIDATE,
            Some(candidate_digest.clone()),
            Some(candidate_digest),
            "advisory",
            vec!["current provider ref is not an immutable sha256 digest".to_string()],
            None,
        );
    }
    let status = if current_ref.as_deref() == Some(candidate_digest.as_str())
        && record.artifact.digest == candidate_digest
    {
        STATUS_UP_TO_DATE
    } else {
        STATUS_OUTDATED
    };
    status_row(
        record,
        status,
        Some(candidate_digest.clone()),
        Some(candidate_digest),
        "immutable",
        risk_for_status(status),
        None,
    )
}

fn git_provider_outdated_row(
    ctx: &AppContext,
    record: &SkillSourceRecord,
    staging_root: &Path,
) -> ProviderOutdatedRow {
    let Some(remote) = git_remote_for_source(&record.source) else {
        return status_row(
            record,
            STATUS_INVALID_SOURCE,
            None,
            None,
            "unavailable",
            vec!["git provider source cannot be resolved to a clone URL".to_string()],
            Some("missing or unsupported git repository metadata".to_string()),
        );
    };
    let candidate_ref = match resolve_git_head(&remote) {
        Ok(commit) => commit,
        Err(error) => {
            return status_row(
                record,
                STATUS_UNREACHABLE,
                None,
                None,
                "unavailable",
                vec!["git provider head could not be reached".to_string()],
                Some(error),
            );
        }
    };
    let candidate_digest =
        match git_candidate_digest(ctx, record, &remote, &candidate_ref, staging_root) {
            Ok(digest) => digest,
            Err(error) => {
                return status_row(
                    record,
                    STATUS_UNREACHABLE,
                    Some(candidate_ref),
                    None,
                    "unavailable",
                    vec!["git provider candidate digest could not be computed".to_string()],
                    Some(error),
                );
            }
        };
    let current_ref = current_ref_for_source(&record.source);
    if !current_ref.as_deref().is_some_and(is_commit_sha) {
        return status_row(
            record,
            STATUS_UNPINNED_CANDIDATE,
            Some(candidate_ref),
            Some(candidate_digest),
            "advisory",
            vec!["current provider ref is not an immutable commit SHA".to_string()],
            None,
        );
    }
    let status = if current_ref.as_deref() == Some(candidate_ref.as_str())
        && record.artifact.digest == candidate_digest
    {
        STATUS_UP_TO_DATE
    } else {
        STATUS_OUTDATED
    };
    status_row(
        record,
        status,
        Some(candidate_ref),
        Some(candidate_digest),
        "immutable",
        risk_for_status(status),
        None,
    )
}

fn status_row(
    record: &SkillSourceRecord,
    status: &'static str,
    candidate_ref: Option<String>,
    candidate_digest: Option<String>,
    candidate_trust: &'static str,
    risk: Vec<String>,
    error: Option<String>,
) -> ProviderOutdatedRow {
    ProviderOutdatedRow {
        skill_id: record.skill_id.clone(),
        provider: record.source.provider.clone(),
        status,
        current_ref: current_ref_for_source(&record.source),
        current_digest: record.artifact.digest.clone(),
        candidate_ref,
        candidate_digest,
        candidate_trust,
        source_locator: record.source.locator.clone(),
        risk,
        next_actions: next_actions_for_status(&record.skill_id, status),
        error,
    }
}

fn current_ref_for_source(source: &SourceDescriptor) -> Option<String> {
    source
        .resolved_commit
        .clone()
        .or_else(|| source.requested_ref.clone())
}

fn risk_for_status(status: &str) -> Vec<String> {
    match status {
        STATUS_OUTDATED => {
            vec!["provider candidate differs from recorded pinned provenance".to_string()]
        }
        STATUS_UP_TO_DATE => Vec::new(),
        _ => vec!["provider candidate requires review before use".to_string()],
    }
}

fn next_actions_for_status(skill: &str, status: &str) -> Vec<String> {
    match status {
        STATUS_UP_TO_DATE => vec![format!("loom skill provenance verify {skill}")],
        STATUS_OUTDATED => vec![
            format!("loom skill provenance outdated {skill} --plan"),
            "review candidate ref and digest before any re-pin apply flow".to_string(),
        ],
        STATUS_UNREACHABLE => vec![
            "check provider connectivity or local source path".to_string(),
            format!("loom skill provenance outdated {skill}"),
        ],
        STATUS_UNPINNED_CANDIDATE => vec![
            "resolve advisory provider data to an immutable ref before apply".to_string(),
            format!("loom skill provenance outdated {skill} --plan"),
        ],
        STATUS_INVALID_SOURCE => vec![
            "repair source provenance or reinstall from an immutable provider locator".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn git_remote_for_source(source: &SourceDescriptor) -> Option<String> {
    let repository = source.repository.as_deref()?;
    if repository.contains("://")
        || repository.starts_with("git@")
        || repository.starts_with('/')
        || Path::new(repository).exists()
    {
        return Some(repository.to_string());
    }
    if repository.contains('/') {
        return Some(format!("https://github.com/{repository}.git"));
    }
    None
}

fn resolve_git_head(remote: &str) -> std::result::Result<String, String> {
    let output = Command::new("git")
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("protocol.file.allow=always")
        .arg("ls-remote")
        .arg(remote)
        .arg("HEAD")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| format!("failed to run git ls-remote: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git ls-remote failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        let Some(candidate) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        if name == "HEAD" && is_commit_sha(candidate) {
            return Ok(candidate.to_string());
        }
    }
    Err("git ls-remote did not return an immutable HEAD commit".to_string())
}

fn git_candidate_digest(
    ctx: &AppContext,
    record: &SkillSourceRecord,
    remote: &str,
    candidate_ref: &str,
    staging_root: &Path,
) -> std::result::Result<String, String> {
    fs::create_dir_all(staging_root).map_err(|err| {
        format!(
            "failed to create candidate staging root '{}': {err}",
            staging_root.display()
        )
    })?;
    let skill_staging = staging_root.join(&record.skill_id);
    fs::create_dir_all(&skill_staging).map_err(|err| {
        format!(
            "failed to create candidate skill staging root '{}': {err}",
            skill_staging.display()
        )
    })?;
    let source = clone_git_source(
        ctx,
        remote,
        Some(candidate_ref),
        record.source.subdir.clone(),
        &skill_staging,
        |commit, tree| SourceDescriptor {
            provider: record.source.provider.clone(),
            locator: record.source.locator.clone(),
            repository: record.source.repository.clone(),
            path: record.source.path.clone(),
            subdir: record.source.subdir.clone(),
            requested_ref: Some(candidate_ref.to_string()),
            resolved_commit: Some(commit),
            tree_sha: Some(tree),
        },
    )
    .map_err(|err| err.message)?;
    skill_tree_digest(&source.copy_source).map_err(|err| err.to_string())
}

fn re_pin_plan_for_rows(rows: &[ProviderOutdatedRow], generated_at: DateTime<Utc>) -> Value {
    let items: Vec<Value> = rows
        .iter()
        .filter(|row| {
            matches!(row.status, STATUS_OUTDATED | STATUS_UNPINNED_CANDIDATE)
                && row.candidate_ref.is_some()
                && row.candidate_digest.is_some()
        })
        .map(|row| {
            let candidate_ref = row.candidate_ref.as_deref().unwrap_or_default();
            json!({
                "skill_id": row.skill_id,
                "provider": row.provider,
                "status": row.status,
                "mutates": false,
                "apply_deferred": true,
                "current": {
                    "ref": row.current_ref,
                    "digest": row.current_digest,
                },
                "candidate": {
                    "ref": row.candidate_ref,
                    "digest": row.candidate_digest,
                    "trust": row.candidate_trust,
                    "locator": replace_locator_ref(&row.source_locator, candidate_ref),
                },
                "review_gates": [
                    "review provider candidate ref and digest",
                    "run catalog preview or equivalent source inspection",
                    "run skill scan after any explicit update flow",
                ],
                "next_actions": row.next_actions,
            })
        })
        .collect();
    json!({
        "schema_version": 1,
        "plan_id": format!("provider-repin-{}", generated_at.timestamp_micros()),
        "generated_at": generated_at,
        "mutates": false,
        "apply_required": true,
        "items": items,
        "review_gates": [
            "candidate refs are advisory until reviewed",
            "no skill content, provenance, lockfile, projection, or target directory is changed by this plan",
        ],
    })
}

fn replace_locator_ref(locator: &str, candidate_ref: &str) -> String {
    let base = locator
        .rsplit_once('@')
        .map(|(before, _)| before)
        .unwrap_or(locator);
    format!("{base}@{candidate_ref}")
}

fn is_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256_ref(value: &str) -> bool {
    let Some(digest) = value.strip_prefix("sha256:") else {
        return false;
    };
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}
