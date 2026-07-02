use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::ProvisionApplyArgs;
use crate::commands::CommandFailure;
use crate::commands::helpers::{map_git, map_io, shell_arg, validate_non_empty};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::artifact::load_provision_plan_artifact;
use super::model::{ProvisionFilePlan, ProvisionPlan};
use super::utils::{digest_bytes, digest_json, digest_str, normalize_clone_url};

const APPLY_RECORD_SCHEMA: &str = "provision-apply-record-v1";
const DEFAULT_APPROVAL: &str = "approval:provision-apply";

struct TargetValidation {
    all_content_present: bool,
    errors: Vec<Value>,
}

struct ApplyRecordLock {
    path: PathBuf,
}

impl Drop for ApplyRecordLock {
    fn drop(&mut self) {
        match fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => eprintln!(
                "failed to remove provision apply lock {}: {}",
                self.path.display(),
                err
            ),
        }
    }
}

pub(super) fn cmd_provision_apply(
    ctx: &AppContext,
    args: &ProvisionApplyArgs,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    validate_non_empty("idempotency-key", &args.idempotency_key)?;
    let plan = load_apply_plan(&args.plan)?;
    ensure_supported_plan(&plan)?;
    validate_approvals(&plan, &args.approvals)?;

    let plan_digest = digest_json(&plan)?;
    let key_digest = digest_str(&args.idempotency_key);
    let record_path = apply_record_path(ctx, &key_digest);
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &plan_digest, &key_digest, &record);
    }

    validate_plan_guards(ctx, &plan)?;
    let _record_lock = claim_apply_record_lock(&record_path, &key_digest)?;
    if record_path.is_file() {
        let record = load_apply_record(&record_path)?;
        return replay_existing_apply(&plan, &plan_digest, &key_digest, &record);
    }

    let workspace = PathBuf::from(&plan.workspace);
    let validation = validate_targets(&workspace, &plan.files_to_write)?;
    if !validation.errors.is_empty() {
        if validation.all_content_present {
            write_apply_record(ctx, &record_path, &plan, &plan_digest, &key_digest)?;
            return Ok(apply_response(&plan, &key_digest, true, false, Vec::new()));
        }
        return Err(policy_blocked(
            "provision apply target preimage validation failed",
            json!({
                "plan_id": plan.plan_id,
                "errors": validation.errors,
                "target_writes_performed": false,
            }),
        ));
    }

    let mut written_files = Vec::new();
    for file in &plan.files_to_write {
        let current_digest = current_target_digest(&workspace, &file.path).map_err(|error| {
            policy_blocked(
                "provision apply target preimage validation failed",
                json!({
                    "plan_id": plan.plan_id,
                    "errors": [error],
                    "target_writes_performed": false,
                }),
            )
        })?;
        if current_digest.as_deref() == Some(file.content_digest.as_str()) {
            continue;
        }
        if let Some(error) = target_preimage_error(file, current_digest.as_deref()) {
            return Err(policy_blocked(
                "provision apply target preimage validation failed",
                json!({
                    "plan_id": plan.plan_id,
                    "errors": [error],
                    "target_writes_performed": false,
                }),
            ));
        }
        ensure_target_parent_chain(&workspace, &file.path)?;
        if current_target_digest(&workspace, &file.path)
            .map_err(|error| {
                policy_blocked(
                    "provision apply target preimage validation failed",
                    json!({
                        "plan_id": plan.plan_id,
                        "errors": [error],
                        "target_writes_performed": false,
                    }),
                )
            })?
            .as_deref()
            != current_digest.as_deref()
        {
            return Err(policy_blocked(
                "provision apply target changed during apply; retry with a new reviewed plan",
                json!({
                    "plan_id": plan.plan_id,
                    "path": file.path,
                    "target_writes_performed": false,
                }),
            ));
        }
        let absolute = workspace.join(&file.path);
        if current_digest.as_deref() == Some(file.content_digest.as_str()) {
            continue;
        }
        write_atomic(&absolute, &file.preview).map_err(map_io)?;
        written_files.push(file.path.clone());
    }
    write_apply_record(ctx, &record_path, &plan, &plan_digest, &key_digest)?;

    Ok(apply_response(
        &plan,
        &key_digest,
        false,
        !written_files.is_empty(),
        written_files,
    ))
}

fn load_apply_plan(raw: &str) -> std::result::Result<ProvisionPlan, CommandFailure> {
    if Path::new(raw).is_file() {
        return load_provision_plan_artifact(raw);
    }
    Err(policy_blocked(
        "provision apply currently requires an explicit reviewed plan artifact path",
        json!({
            "plan": raw,
            "target_writes_performed": false,
        }),
    ))
}

fn ensure_supported_plan(plan: &ProvisionPlan) -> std::result::Result<(), CommandFailure> {
    if plan.target_kind != "devcontainer" {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!(
                "provision apply does not support target '{}'",
                plan.target_kind
            ),
        ));
    }
    Ok(())
}

fn validate_plan_guards(
    ctx: &AppContext,
    plan: &ProvisionPlan,
) -> std::result::Result<(), CommandFailure> {
    require_guard_str(plan, "root", ctx.root.display().to_string())?;
    require_guard_str(plan, "active_view_digest", digest_json(&plan.active_views)?)?;
    require_guard_str(
        plan,
        "dependency_readiness_digest",
        digest_json(&plan.dependency_readiness)?,
    )?;
    require_guard_str(plan, "files_digest", digest_json(&plan.files_to_write)?)?;
    validate_redacted_registry_reference(plan)?;

    let reviewed_head = guard_string(plan, "registry_head")?;
    let reachable = plan
        .guards
        .get("registry_head_reachable")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !reachable || reviewed_head == "working-tree" {
        return Err(policy_blocked(
            "provision apply requires a reachable reviewed registry head",
            json!({
                "registry_head": reviewed_head,
                "registry_head_reachable": reachable,
                "target_writes_performed": false,
            }),
        ));
    }
    let current_head = gitops::head(ctx).map_err(map_git)?;
    if current_head != reviewed_head {
        return Err(policy_blocked(
            "provision plan registry head is stale; create a new provision plan",
            json!({
                "expected": reviewed_head,
                "actual": current_head,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn validate_redacted_registry_reference(
    plan: &ProvisionPlan,
) -> std::result::Result<(), CommandFailure> {
    match (
        &plan.registry_clone_url,
        plan.registry_source_display.as_str(),
    ) {
        (None, "local-only") => Ok(()),
        (Some(clone_url), display) if display == clone_url => {
            validate_redacted_registry_url("registry_clone_url", clone_url)
        }
        (None, display) => Err(policy_blocked(
            "provision apply requires a credential-redacted registry clone URL",
            json!({
                "registry_source_display": display,
                "registry_clone_url_present": false,
                "target_writes_performed": false,
            }),
        )),
        (Some(_), display) => Err(policy_blocked(
            "provision apply registry display must match the reviewed clone URL",
            json!({
                "registry_source_display": display,
                "registry_clone_url_present": true,
                "target_writes_performed": false,
            }),
        )),
    }
}

fn validate_redacted_registry_url(
    field: &str,
    url: &str,
) -> std::result::Result<(), CommandFailure> {
    let normalized = normalize_clone_url(url);
    if normalized.local_only
        || normalized.secret_redacted
        || normalized.clone_url.as_deref() != Some(url)
        || normalized.display != url
    {
        return Err(policy_blocked(
            "provision apply requires a credential-redacted registry clone URL",
            json!({
                "field": field,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn require_guard_str(
    plan: &ProvisionPlan,
    key: &str,
    expected: String,
) -> std::result::Result<(), CommandFailure> {
    let actual = guard_string(plan, key)?;
    if actual != expected {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            format!("provision plan guard '{key}' does not match reviewed content"),
        ));
    }
    Ok(())
}

fn guard_string(plan: &ProvisionPlan, key: &str) -> std::result::Result<String, CommandFailure> {
    plan.guards
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("provision plan is missing {key} guard"),
            )
        })
}

fn validate_approvals(
    plan: &ProvisionPlan,
    provided: &[String],
) -> std::result::Result<(), CommandFailure> {
    let required = required_approvals(plan);
    let provided = provided.iter().cloned().collect::<BTreeSet<_>>();
    let missing = required
        .iter()
        .filter(|approval| !provided.contains(*approval))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(policy_blocked(
            "provision apply requires approval token(s)",
            json!({
                "required_approvals": required,
                "missing_approvals": missing,
                "target_writes_performed": false,
            }),
        ));
    }
    Ok(())
}

fn required_approvals(plan: &ProvisionPlan) -> Vec<String> {
    let approval_required = plan
        .policy
        .get("approval_required_for_apply")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !approval_required {
        return Vec::new();
    }
    let approvals = plan
        .policy
        .get("required_approvals")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if approvals.is_empty() {
        vec![DEFAULT_APPROVAL.to_string()]
    } else {
        approvals
    }
}

fn validate_targets(
    workspace: &Path,
    files: &[ProvisionFilePlan],
) -> std::result::Result<TargetValidation, CommandFailure> {
    let mut errors = Vec::new();
    let mut all_content_present = true;
    for file in files {
        validate_target_path(&file.path)?;
        let reviewed_digest = digest_str(&file.preview);
        if reviewed_digest != file.content_digest {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "plan file '{}' digest does not match reviewed content",
                    file.path
                ),
            ));
        }
        let current_digest = match current_target_digest(workspace, &file.path) {
            Ok(digest) => digest,
            Err(error) => {
                all_content_present = false;
                errors.push(error);
                continue;
            }
        };
        if current_digest.as_deref() != Some(file.content_digest.as_str()) {
            all_content_present = false;
        }
        if let Some(error) = target_preimage_error(file, current_digest.as_deref()) {
            errors.push(error);
        }
    }
    Ok(TargetValidation {
        all_content_present,
        errors,
    })
}

fn current_target_digest(
    workspace: &Path,
    path: &str,
) -> std::result::Result<Option<String>, Value> {
    validate_target_parent_chain(workspace, path)?;
    let absolute = workspace.join(path);
    match fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(target_error(
            path,
            "target path is a symlink",
            json!({"path": absolute.display().to_string()}),
        )),
        Ok(metadata) if !metadata.is_file() => Err(target_error(
            path,
            "target path exists but is not a regular file",
            json!({"path": absolute.display().to_string()}),
        )),
        Ok(_) => fs::read(&absolute)
            .map(|raw| Some(digest_bytes(&raw)))
            .map_err(|err| {
                target_error(
                    path,
                    "target file could not be read for preimage validation",
                    json!({"path": absolute.display().to_string(), "error": err.to_string()}),
                )
            }),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(target_error(
            path,
            "target path could not be inspected",
            json!({"path": absolute.display().to_string(), "error": err.to_string()}),
        )),
    }
}

fn validate_target_parent_chain(workspace: &Path, path: &str) -> std::result::Result<(), Value> {
    validate_workspace_root(workspace, path)?;
    let mut cursor = workspace.to_path_buf();
    let mut components = Path::new(path).components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            continue;
        };
        if components.peek().is_none() {
            break;
        }
        cursor.push(name);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(target_error(
                    path,
                    "target parent path is a symlink",
                    json!({"path": cursor.display().to_string()}),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(target_error(
                    path,
                    "target parent path exists but is not a directory",
                    json!({"path": cursor.display().to_string()}),
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(target_error(
                    path,
                    "target parent path could not be inspected",
                    json!({"path": cursor.display().to_string(), "error": err.to_string()}),
                ));
            }
        }
    }
    Ok(())
}

fn ensure_target_parent_chain(
    workspace: &Path,
    path: &str,
) -> std::result::Result<(), CommandFailure> {
    validate_workspace_root(workspace, path).map_err(|error| {
        policy_blocked(
            "provision apply target preimage validation failed",
            json!({
                "errors": [error],
                "target_writes_performed": false,
            }),
        )
    })?;
    let mut cursor = workspace.to_path_buf();
    let mut components = Path::new(path).components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            continue;
        };
        if components.peek().is_none() {
            break;
        }
        cursor.push(name);
        loop {
            match fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(policy_blocked(
                        "provision apply target preimage validation failed",
                        json!({
                            "errors": [target_error(
                                path,
                                "target parent path is a symlink",
                                json!({"path": cursor.display().to_string()}),
                            )],
                            "target_writes_performed": false,
                        }),
                    ));
                }
                Ok(metadata) if !metadata.is_dir() => {
                    return Err(policy_blocked(
                        "provision apply target preimage validation failed",
                        json!({
                            "errors": [target_error(
                                path,
                                "target parent path exists but is not a directory",
                                json!({"path": cursor.display().to_string()}),
                            )],
                            "target_writes_performed": false,
                        }),
                    ));
                }
                Ok(_) => break,
                Err(err) if err.kind() == ErrorKind::NotFound => match fs::create_dir(&cursor) {
                    Ok(()) => break,
                    Err(err) if err.kind() == ErrorKind::AlreadyExists => continue,
                    Err(err) => return Err(map_io(err)),
                },
                Err(err) => return Err(map_io(err)),
            }
        }
    }
    Ok(())
}

fn validate_workspace_root(workspace: &Path, path: &str) -> std::result::Result<(), Value> {
    match fs::symlink_metadata(workspace) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(target_error(
            path,
            "workspace path is a symlink",
            json!({"path": workspace.display().to_string()}),
        )),
        Ok(metadata) if !metadata.is_dir() => Err(target_error(
            path,
            "workspace path exists but is not a directory",
            json!({"path": workspace.display().to_string()}),
        )),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == ErrorKind::NotFound => Err(target_error(
            path,
            "workspace path does not exist",
            json!({"path": workspace.display().to_string()}),
        )),
        Err(err) => Err(target_error(
            path,
            "workspace path could not be inspected",
            json!({"path": workspace.display().to_string(), "error": err.to_string()}),
        )),
    }
}

fn target_preimage_error(file: &ProvisionFilePlan, current_digest: Option<&str>) -> Option<Value> {
    if !file.safe_to_apply && current_digest != Some(file.content_digest.as_str()) {
        return Some(target_error(
            &file.path,
            "plan marked file unsafe to apply",
            json!({}),
        ));
    }
    match (&file.preimage_digest, current_digest) {
        (None, None) => None,
        (None, Some(current)) if current == file.content_digest => None,
        (None, Some(current)) => Some(target_error(
            &file.path,
            "target file exists but reviewed plan expected it to be absent",
            json!({"actual": current}),
        )),
        (Some(expected), Some(current)) if current == expected => None,
        (Some(_expected), Some(current)) if current == file.content_digest => None,
        (Some(expected), actual) => Some(target_error(
            &file.path,
            "target file changed since reviewed plan",
            json!({"expected": expected, "actual": actual}),
        )),
    }
}

fn target_error(path: &str, reason: &str, details: Value) -> Value {
    json!({
        "path": path,
        "reason": reason,
        "details": details,
    })
}

fn validate_target_path(path: &str) -> std::result::Result<(), CommandFailure> {
    if path.is_empty() {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provision target path must not be empty",
        ));
    }
    for component in Path::new(path).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(CommandFailure::new(
                    ErrorCode::ArgInvalid,
                    format!("unsafe provision target path {path}"),
                ));
            }
        }
    }
    Ok(())
}

fn replay_existing_apply(
    plan: &ProvisionPlan,
    plan_digest: &str,
    key_digest: &str,
    record: &Value,
) -> std::result::Result<(Value, Meta), CommandFailure> {
    if record.get("schema_version").and_then(Value::as_str) != Some(APPLY_RECORD_SCHEMA)
        || record.get("plan_digest").and_then(Value::as_str) != Some(plan_digest)
        || record.get("idempotency_key_digest").and_then(Value::as_str) != Some(key_digest)
    {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "provision idempotency key was already used for a different plan",
        ));
    }
    for file in &plan.files_to_write {
        validate_target_path(&file.path)?;
        let current_digest = current_target_digest(Path::new(&plan.workspace), &file.path)
            .map_err(|error| {
                policy_blocked(
                    "provision apply replay target files no longer match the reviewed plan",
                    json!({
                        "plan_id": plan.plan_id,
                        "errors": [error],
                        "target_writes_performed": false,
                    }),
                )
            })?;
        if current_digest.as_deref() != Some(file.content_digest.as_str()) {
            return Err(policy_blocked(
                "provision apply replay target files no longer match the reviewed plan",
                json!({
                    "plan_id": plan.plan_id,
                    "path": file.path,
                    "target_writes_performed": false,
                }),
            ));
        }
    }
    Ok(apply_response(plan, key_digest, true, false, Vec::new()))
}

fn load_apply_record(path: &Path) -> std::result::Result<Value, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))
}

fn claim_apply_record_lock(
    record_path: &Path,
    key_digest: &str,
) -> std::result::Result<ApplyRecordLock, CommandFailure> {
    let lock_path = record_path.with_extension("lock");
    fs::create_dir_all(lock_path.parent().unwrap_or(Path::new("."))).map_err(map_io)?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(mut file) => {
            writeln!(file, "{key_digest}").map_err(map_io)?;
            file.sync_all().map_err(map_io)?;
            Ok(ApplyRecordLock { path: lock_path })
        }
        Err(err) if err.kind() == ErrorKind::AlreadyExists => Err(policy_blocked(
            "provision apply idempotency key is already in progress",
            json!({
                "idempotency_key_digest": key_digest,
                "target_writes_performed": false,
            }),
        )),
        Err(err) => Err(map_io(err)),
    }
}

fn write_apply_record(
    ctx: &AppContext,
    path: &Path,
    plan: &ProvisionPlan,
    plan_digest: &str,
    key_digest: &str,
) -> std::result::Result<(), CommandFailure> {
    let record = json!({
        "schema_version": APPLY_RECORD_SCHEMA,
        "plan_id": plan.plan_id,
        "plan_digest": plan_digest,
        "idempotency_key_digest": key_digest,
        "target_kind": plan.target_kind,
        "workspace": plan.workspace,
        "files": plan.files_to_write.iter().map(|file| json!({
            "path": file.path,
            "content_digest": file.content_digest,
        })).collect::<Vec<_>>(),
    });
    fs::create_dir_all(path.parent().unwrap_or(&ctx.state_dir)).map_err(map_io)?;
    let mut raw = serde_json::to_string_pretty(&record).map_err(map_io)?;
    raw.push('\n');
    write_atomic(path, &raw).map_err(map_io)
}

fn apply_record_path(ctx: &AppContext, key_digest: &str) -> PathBuf {
    let suffix = key_digest
        .strip_prefix("sha256:")
        .unwrap_or(key_digest)
        .replace(['/', '\\', ':'], "_");
    ctx.state_dir
        .join("provision")
        .join("applies")
        .join(format!("{suffix}.json"))
}

fn apply_response(
    plan: &ProvisionPlan,
    key_digest: &str,
    idempotent_replay: bool,
    target_writes_performed: bool,
    written_files: Vec<String>,
) -> (Value, Meta) {
    (
        json!({
            "plan_id": plan.plan_id,
            "target_kind": plan.target_kind,
            "workspace": plan.workspace,
            "idempotency_key_digest": key_digest,
            "idempotent_replay": idempotent_replay,
            "target_writes_performed": target_writes_performed,
            "written_files": written_files,
            "applied_files": plan.files_to_write.iter().map(|file| json!({
                "path": file.path,
                "content_digest": file.content_digest,
            })).collect::<Vec<_>>(),
            "recovery": recovery(plan),
        }),
        Meta::default(),
    )
}

fn recovery(plan: &ProvisionPlan) -> Value {
    let commands = plan
        .files_to_write
        .iter()
        .filter(|file| file.preimage_digest.is_none())
        .map(|file| {
            let path = Path::new(&plan.workspace).join(&file.path);
            format!("rm -f {}", shell_arg(&path))
        })
        .collect::<Vec<_>>();
    json!({
        "rollback_supported": !commands.is_empty(),
        "commands": commands,
        "note": "files with reviewed preimages should be restored from VCS or backup if manual rollback is required",
    })
}

fn policy_blocked(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
