use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::commands::CommandFailure;
use crate::commands::helpers::map_io;
use crate::types::ErrorCode;

use super::model::{PROVISION_PLAN_SCHEMA, ProvisionPlan};
use super::utils::digest_str;

const SHELL_ARTIFACT_MAGIC: &str = "# loom-provision-artifact-v1";
const SHELL_ARTIFACT_SCHEMA: &str = "provision-shell-artifact-v1";

pub(super) struct ShellExportArtifact {
    pub schema_version: &'static str,
    pub body: String,
    pub source_path: String,
    pub content_digest: String,
}

pub(super) struct ProvisionArtifactInspection {
    pub kind: &'static str,
    pub schema_version: String,
    pub plan_id: String,
    pub target_kind: String,
    pub source_path: Option<String>,
    pub content_digest: Option<String>,
    pub script_bytes: Option<usize>,
    pub checksums_verified: bool,
    pub entry_count: usize,
    pub generated_file_count: usize,
    pub registry_file_count: usize,
    pub active_view_file_count: usize,
    pub planned_files: Value,
}

pub(super) fn load_provision_plan_artifact(plan: &str) -> Result<ProvisionPlan, CommandFailure> {
    let path = Path::new(plan);
    if !path.is_file() {
        return Err(deferred_plan_id(plan));
    }
    let raw = fs::read_to_string(path).map_err(map_io)?;
    let plan: ProvisionPlan = serde_json::from_str(&raw)
        .map_err(|err| CommandFailure::new(ErrorCode::ArgInvalid, err.to_string()))?;
    if plan.schema_version != PROVISION_PLAN_SCHEMA {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "unsupported provision plan schema_version {}",
                plan.schema_version
            ),
        ));
    }
    Ok(plan)
}

pub(super) fn build_shell_export_artifact(
    plan: &ProvisionPlan,
) -> Result<ShellExportArtifact, CommandFailure> {
    let setup = plan
        .files_to_write
        .iter()
        .find(|file| file.kind == "shell" && file.path == ".devcontainer/loom-setup.sh")
        .ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::ArgInvalid,
                "plan does not contain a reviewed devcontainer setup shell file",
            )
        })?;
    let content_digest = digest_str(&setup.preview);
    if content_digest != setup.content_digest {
        return Err(CommandFailure::new(
            ErrorCode::ArgInvalid,
            "plan setup script digest does not match reviewed content",
        ));
    }

    let body = format!(
        "{magic}\n\
         # schema_version={schema}\n\
         # plan_schema_version={plan_schema}\n\
         # plan_id={plan_id}\n\
         # target_kind={target_kind}\n\
         # source_path={source_path}\n\
         # content_digest={content_digest}\n\
         # registry_head={registry_head}\n\
         # active_view_digest={active_view_digest}\n\
         # target_writes_performed=false\n\
         \n\
         {script}",
        magic = SHELL_ARTIFACT_MAGIC,
        schema = SHELL_ARTIFACT_SCHEMA,
        plan_schema = plan.schema_version,
        plan_id = plan.plan_id,
        target_kind = plan.target_kind,
        source_path = setup.path,
        content_digest = content_digest,
        registry_head = header_value(&plan.guards, "registry_head"),
        active_view_digest = header_value(&plan.guards, "active_view_digest"),
        script = setup.preview,
    );

    Ok(ShellExportArtifact {
        schema_version: SHELL_ARTIFACT_SCHEMA,
        body,
        source_path: setup.path.clone(),
        content_digest,
    })
}

pub(super) fn inspect_provision_artifact(
    path: &Path,
) -> Result<ProvisionArtifactInspection, CommandFailure> {
    if let Ok(raw) = fs::read_to_string(path)
        && raw.starts_with(SHELL_ARTIFACT_MAGIC)
    {
        return inspect_shell_export_artifact_text(&raw);
    }
    super::tar_artifact::inspect_tar_export_artifact(path)
}

fn inspect_shell_export_artifact_text(
    raw: &str,
) -> Result<ProvisionArtifactInspection, CommandFailure> {
    let Some((header, script)) = raw.split_once("\n\n") else {
        return Err(invalid_artifact(
            "shell artifact is missing a header/script separator",
        ));
    };
    let mut lines = header.lines();
    if lines.next() != Some(SHELL_ARTIFACT_MAGIC) {
        return Err(invalid_artifact(
            "artifact is not a Loom provision shell artifact",
        ));
    }

    let mut metadata = BTreeMap::new();
    for line in lines {
        let Some(body) = line.strip_prefix("# ") else {
            return Err(invalid_artifact(
                "shell artifact header contains a malformed line",
            ));
        };
        let Some((key, value)) = body.split_once('=') else {
            return Err(invalid_artifact(
                "shell artifact header contains a malformed field",
            ));
        };
        metadata.insert(key.to_string(), value.to_string());
    }

    let schema_version = required_metadata(&metadata, "schema_version")?;
    if schema_version != SHELL_ARTIFACT_SCHEMA {
        return Err(invalid_artifact(
            "unsupported shell artifact schema version",
        ));
    }
    let content_digest = required_metadata(&metadata, "content_digest")?;
    if digest_str(script) != content_digest {
        return Err(invalid_artifact(
            "shell artifact script digest does not match metadata",
        ));
    }

    Ok(ProvisionArtifactInspection {
        kind: "shell",
        schema_version,
        plan_id: required_metadata(&metadata, "plan_id")?,
        target_kind: required_metadata(&metadata, "target_kind")?,
        source_path: Some(required_metadata(&metadata, "source_path")?),
        content_digest: Some(content_digest.clone()),
        script_bytes: Some(script.len()),
        checksums_verified: false,
        entry_count: 1,
        generated_file_count: 1,
        registry_file_count: 0,
        active_view_file_count: 0,
        planned_files: json!([{
            "path": required_metadata(&metadata, "source_path")?,
            "kind": "shell",
            "content_digest": content_digest,
            "action": "review_only",
        }]),
    })
}

fn header_value(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string()
}

fn required_metadata(
    metadata: &BTreeMap<String, String>,
    key: &str,
) -> Result<String, CommandFailure> {
    metadata
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| invalid_artifact(format!("shell artifact is missing {key} metadata")))
}

fn invalid_artifact(message: impl Into<String>) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, message.into())
}

fn deferred_plan_id(plan: &str) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::PolicyBlocked,
        "provision export currently requires an explicit reviewed plan artifact path",
    );
    failure.details = json!({
        "plan": plan,
        "target_writes_performed": false,
    });
    failure
}
