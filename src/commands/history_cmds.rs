use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;

use serde_json::{Value, json};

use crate::cli::HistoryArgs;
use crate::envelope::Meta;
use crate::gitops;
use crate::state_model::RegistryOperationRecord;
use crate::types::ErrorCode;

use super::helpers::{map_arg, map_git, validate_skill_name};
use super::{App, CommandFailure};

impl App {
    pub fn cmd_history(
        &self,
        args: &HistoryArgs,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        if args.limit == 0 {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                "--limit must be greater than 0",
            ));
        }
        validate_history_ref_arg("to", &args.to)?;
        if let Some(from) = &args.from {
            validate_history_ref_arg("from", from)?;
        }
        if !gitops::repo_is_initialized(&self.ctx).map_err(map_git)? {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "registry root '{}' is not a Git repository",
                    self.ctx.root.display()
                ),
            ));
        }

        let skill_rel = format!("skills/{}", args.skill);
        let range = args
            .from
            .as_ref()
            .map(|from| format!("{}..{}", from, args.to))
            .unwrap_or_else(|| args.to.clone());
        let commits = skill_history_commits(&self.ctx, &skill_rel, &range, args.limit)?;
        if commits.is_empty() && !self.ctx.skill_path(&args.skill).exists() {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skill '{}' not found", args.skill),
            ));
        }

        let refs_by_commit = skill_history_refs(&self.ctx, &args.skill)?;
        let mut meta = Meta::default();
        let operations_by_effect_commit = if args.include_ops {
            current_operations_by_effect_commit(&self.ctx, &args.skill, &mut meta)
        } else {
            BTreeMap::new()
        };
        let items = commits
            .into_iter()
            .map(|commit| {
                let refs = refs_by_commit
                    .get(&commit.commit)
                    .cloned()
                    .unwrap_or_default();
                let operations = if args.include_ops {
                    let mut operations = operations_added_by_commit(
                        &self.ctx,
                        &commit.commit,
                        &args.skill,
                        &mut meta,
                    );
                    if let Some(by_effect_commit) = operations_by_effect_commit.get(&commit.commit)
                    {
                        append_missing_operations(&mut operations, by_effect_commit);
                    }
                    operations
                } else {
                    Vec::new()
                };
                let diff_stat = if args.include_diff_stat {
                    Some(skill_history_diff_stat(
                        &self.ctx,
                        &commit.commit,
                        &skill_rel,
                        &mut meta,
                    ))
                } else {
                    None
                };
                let mut item = json!({
                    "commit": commit.commit,
                    "short_commit": commit.short_commit,
                    "author_name": commit.author_name,
                    "author_email": commit.author_email,
                    "committed_at": commit.committed_at,
                    "message": commit.message,
                    "refs": refs,
                    "operations": operations,
                });
                if let Some(diff_stat) = diff_stat {
                    item["diff_stat"] = diff_stat;
                }
                item
            })
            .collect::<Vec<_>>();

        Ok((
            json!({
                "skill": args.skill,
                "range": {
                    "from": args.from,
                    "to": args.to,
                },
                "items": items,
            }),
            meta,
        ))
    }
}

fn validate_history_ref_arg(name: &str, value: &str) -> std::result::Result<(), CommandFailure> {
    if is_safe_history_ref(value) {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::ArgInvalid,
        format!("--{} must be a safe Git revision", name),
    ))
}

fn is_safe_history_ref(value: &str) -> bool {
    let len = value.len();
    !value.is_empty()
        && len <= 256
        && !value.starts_with('-')
        && !value.contains("..")
        && value.bytes().all(|byte| {
            matches!(
                byte,
                b'a'..=b'z'
                    | b'A'..=b'Z'
                    | b'0'..=b'9'
                    | b'.'
                    | b'_'
                    | b'-'
                    | b'/'
                    | b'~'
                    | b'^'
            )
        })
}

#[derive(Debug)]
struct SkillHistoryCommit {
    commit: String,
    short_commit: String,
    author_name: String,
    author_email: String,
    committed_at: String,
    message: String,
}

fn skill_history_commits(
    ctx: &crate::state::AppContext,
    skill_rel: &str,
    range: &str,
    limit: usize,
) -> std::result::Result<Vec<SkillHistoryCommit>, CommandFailure> {
    let limit_arg = format!("-n{}", limit);
    let output = gitops::run_git(
        ctx,
        &[
            "log",
            &limit_arg,
            "--date=iso-strict",
            "--format=%H%x1f%h%x1f%an%x1f%ae%x1f%aI%x1f%s%x1e",
            range,
            "--",
            skill_rel,
        ],
    )
    .map_err(map_git)?;
    let mut commits = Vec::new();
    for record in output.split('\x1e') {
        let record = record.trim();
        if record.is_empty() {
            continue;
        }
        let fields = record.split('\x1f').collect::<Vec<_>>();
        if fields.len() != 6 {
            return Err(CommandFailure::new(
                ErrorCode::InternalError,
                format!("failed to parse git history record for {}", skill_rel),
            ));
        }
        commits.push(SkillHistoryCommit {
            commit: fields[0].to_string(),
            short_commit: fields[1].to_string(),
            author_name: fields[2].to_string(),
            author_email: fields[3].to_string(),
            committed_at: fields[4].to_string(),
            message: fields[5].to_string(),
        });
    }
    Ok(commits)
}

fn skill_history_refs(
    ctx: &crate::state::AppContext,
    skill: &str,
) -> std::result::Result<BTreeMap<String, Vec<String>>, CommandFailure> {
    let output = gitops::run_git(
        ctx,
        &[
            "for-each-ref",
            "--format=%(objectname)\t%(*objectname)\t%(refname:short)",
            "refs/tags/snapshot",
            "refs/tags/release",
            "refs/tags/recovery",
        ],
    )
    .map_err(map_git)?;

    let mut refs_by_commit: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in output.lines() {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            continue;
        }
        if !ref_belongs_to_skill(fields[2], skill) {
            continue;
        }
        let commit = if fields[1].is_empty() {
            fields[0]
        } else {
            fields[1]
        };
        refs_by_commit
            .entry(commit.to_string())
            .or_default()
            .push(fields[2].to_string());
    }
    Ok(refs_by_commit)
}

fn ref_belongs_to_skill(reference: &str, skill: &str) -> bool {
    reference.starts_with(&format!("snapshot/{}/", skill))
        || reference.starts_with(&format!("release/{}/", skill))
        || reference.starts_with(&format!("recovery/{}/", skill))
}

fn operations_added_by_commit(
    ctx: &crate::state::AppContext,
    commit: &str,
    skill: &str,
    meta: &mut Meta,
) -> Vec<Value> {
    let output = match gitops::run_git(
        ctx,
        &[
            "show",
            "--format=",
            "--no-ext-diff",
            commit,
            "--",
            "state/registry/ops/operations.jsonl",
        ],
    ) {
        Ok(output) => output,
        Err(err) => {
            meta.warnings.push(format!(
                "failed to read registry operations for commit {}: {}",
                commit, err
            ));
            return Vec::new();
        }
    };

    let mut operations = Vec::new();
    for line in output.lines() {
        if !line.starts_with('+') || line.starts_with("+++") {
            continue;
        }
        let raw = &line[1..];
        if raw.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RegistryOperationRecord>(raw) {
            Ok(record) if operation_mentions_skill(&record, skill) => {
                operations.push(operation_json(&record));
            }
            Ok(_) => {}
            Err(err) => meta.warnings.push(format!(
                "skipped malformed registry operation in commit {}: {}",
                commit, err
            )),
        }
    }
    operations
}

fn current_operations_by_effect_commit(
    ctx: &crate::state::AppContext,
    skill: &str,
    meta: &mut Meta,
) -> BTreeMap<String, Vec<Value>> {
    let path = ctx.root.join("state/registry/ops/operations.jsonl");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return BTreeMap::new(),
        Err(err) => {
            meta.warnings.push(format!(
                "failed to read current registry operations at {}: {}",
                path.display(),
                err
            ));
            return BTreeMap::new();
        }
    };

    let mut operations_by_commit: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<RegistryOperationRecord>(line) {
            Ok(record) if operation_mentions_skill(&record, skill) => {
                if let Some(commit) = json_field_str(&record.effects, "commit") {
                    operations_by_commit
                        .entry(commit.to_string())
                        .or_default()
                        .push(operation_json(&record));
                }
            }
            Ok(_) => {}
            Err(err) => meta.warnings.push(format!(
                "skipped malformed registry operation at current log line {}: {}",
                index + 1,
                err
            )),
        }
    }
    operations_by_commit
}

fn append_missing_operations(operations: &mut Vec<Value>, extra_operations: &[Value]) {
    let mut seen = operations
        .iter()
        .filter_map(|operation| {
            operation
                .get("op_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    for operation in extra_operations {
        if let Some(op_id) = operation.get("op_id").and_then(Value::as_str)
            && seen.insert(op_id.to_string())
        {
            operations.push(operation.clone());
        }
    }
}

fn operation_json(record: &RegistryOperationRecord) -> Value {
    json!({
        "op_id": record.op_id,
        "intent": record.intent,
        "created_at": record.created_at,
    })
}

fn operation_mentions_skill(record: &RegistryOperationRecord, skill: &str) -> bool {
    json_field_eq(&record.payload, "skill", skill)
        || json_field_eq(&record.payload, "skill_id", skill)
        || json_field_eq(&record.effects, "skill", skill)
        || json_field_eq(&record.effects, "skill_id", skill)
}

fn json_field_eq(value: &Value, key: &str, expected: &str) -> bool {
    json_field_str(value, key).is_some_and(|actual| actual == expected)
}

fn json_field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn skill_history_diff_stat(
    ctx: &crate::state::AppContext,
    commit: &str,
    skill_rel: &str,
    meta: &mut Meta,
) -> Value {
    let output = match gitops::run_git(
        ctx,
        &["show", "--shortstat", "--format=", commit, "--", skill_rel],
    ) {
        Ok(output) => output,
        Err(err) => {
            meta.warnings.push(format!(
                "failed to read diff stat for commit {}: {}",
                commit, err
            ));
            return json!({
                "files_changed": 0,
                "insertions": 0,
                "deletions": 0,
            });
        }
    };
    parse_shortstat(&output)
}

fn parse_shortstat(output: &str) -> Value {
    let mut files_changed = 0u64;
    let mut insertions = 0u64;
    let mut deletions = 0u64;
    let normalized = output.replace(',', "");
    let tokens = normalized.split_whitespace().collect::<Vec<_>>();
    for window in tokens.windows(2) {
        let Ok(count) = window[0].parse::<u64>() else {
            continue;
        };
        match window[1] {
            "file" | "files" => files_changed = count,
            token if token.starts_with("insertion") => insertions = count,
            token if token.starts_with("deletion") => deletions = count,
            _ => {}
        }
    }
    json!({
        "files_changed": files_changed,
        "insertions": insertions,
        "deletions": deletions,
    })
}
