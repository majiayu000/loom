use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path};

use serde::Deserialize;
use serde_json::json;

use crate::state::AppContext;
use crate::types::ErrorCode;

use super::super::CommandFailure;
use super::super::helpers::map_io;
use super::model::{
    ArtifactStatus, ArtifactVerifyReport, BoundariesDoc, COMPILE_SCHEMA_VERSION, COMPILER_VERSION,
    CatalogDoc, CompiledArtifactManifest, REQUIRED_ARTIFACT_FILES, ReferencesDoc, ToolInterfaceDoc,
    VerifyFinding,
};
use super::plan::source_digest_info;
use super::util::{digest_bytes_prefixed, push_compile_finding, validate_artifact_id};

pub(super) fn verify_artifact(
    ctx: &AppContext,
    skill: &str,
    root: &Path,
    artifact_id: &str,
) -> std::result::Result<ArtifactVerifyReport, CommandFailure> {
    validate_artifact_id(artifact_id)?;
    let artifact_dir = root.join(artifact_id);
    let mut findings = Vec::new();
    if !artifact_dir.is_dir() {
        push_compile_finding(
            &mut findings,
            "artifact_missing",
            "error",
            "artifact directory does not exist",
            json!({"artifact_id": artifact_id}),
        );
        return Ok(report_without_manifest(
            artifact_id,
            &artifact_dir,
            "missing",
            findings,
        ));
    }

    let manifest_path = artifact_dir.join("manifest.json");
    if !manifest_path.is_file() {
        push_compile_finding(
            &mut findings,
            "manifest_missing",
            "error",
            "manifest.json is missing",
            json!({ "path": manifest_path.display().to_string() }),
        );
        return Ok(report_without_manifest(
            artifact_id,
            &artifact_dir,
            "invalid",
            findings,
        ));
    }

    let manifest = load_manifest(&manifest_path)?;
    if manifest.schema_version != COMPILE_SCHEMA_VERSION {
        return Err(compiled_manifest_schema_failure(
            &manifest_path,
            format!(
                "unsupported compiled artifact schema_version {}",
                manifest.schema_version
            ),
        ));
    }
    validate_manifest_identity(skill, artifact_id, &manifest, &mut findings);
    validate_required_files(&artifact_dir, &mut findings);
    validate_content_hashes(&artifact_dir, &manifest, &mut findings);
    validate_sidecars(&artifact_dir, &ctx.skill_path(skill), &mut findings);
    validate_activation_text(&artifact_dir.join("activation.md"), &mut findings);
    let source_stale = validate_source_digest(ctx, skill, &artifact_dir, &manifest, &mut findings)?;
    validate_gate_status(&manifest, &mut findings);

    let valid = findings.is_empty()
        && manifest.status == ArtifactStatus::Valid
        && manifest.gates.all_pass();
    let status = if source_stale {
        ArtifactStatus::Stale.as_str()
    } else if valid {
        ArtifactStatus::Valid.as_str()
    } else {
        manifest.status.as_str()
    };
    Ok(ArtifactVerifyReport {
        artifact_id: artifact_id.to_string(),
        path: artifact_dir.display().to_string(),
        valid,
        status: status.to_string(),
        source_stale,
        findings,
        manifest: Some(manifest),
    })
}

fn report_without_manifest(
    artifact_id: &str,
    artifact_dir: &Path,
    status: &str,
    findings: Vec<VerifyFinding>,
) -> ArtifactVerifyReport {
    ArtifactVerifyReport {
        artifact_id: artifact_id.to_string(),
        path: artifact_dir.display().to_string(),
        valid: false,
        status: status.to_string(),
        source_stale: false,
        findings,
        manifest: None,
    }
}

fn load_manifest(path: &Path) -> std::result::Result<CompiledArtifactManifest, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    serde_json::from_str(&raw)
        .map_err(|err| compiled_manifest_schema_failure(path, err.to_string()))
}

fn compiled_manifest_schema_failure(path: &Path, message: impl Into<String>) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::SchemaMismatch,
        format!("malformed compiled artifact manifest: {}", path.display()),
    );
    failure.details = json!({
        "path": path.display().to_string(),
        "error": message.into(),
    });
    failure
}

fn validate_manifest_identity(
    skill: &str,
    artifact_id: &str,
    manifest: &CompiledArtifactManifest,
    findings: &mut Vec<VerifyFinding>,
) {
    if manifest.compiler_version != COMPILER_VERSION {
        push_compile_finding(
            findings,
            "compiler_version_mismatch",
            "error",
            "compiled artifact uses an unsupported compiler version",
            json!({"expected": COMPILER_VERSION, "actual": manifest.compiler_version.as_str()}),
        );
    }
    if manifest.skill != skill {
        push_compile_finding(
            findings,
            "manifest_skill_mismatch",
            "error",
            "manifest skill does not match the requested skill",
            json!({"expected": skill, "actual": manifest.skill.as_str()}),
        );
    }
    if manifest.artifact_id != artifact_id {
        push_compile_finding(
            findings,
            "manifest_artifact_mismatch",
            "error",
            "manifest artifact_id does not match the artifact directory",
            json!({"expected": artifact_id, "actual": manifest.artifact_id.as_str()}),
        );
    }
}

fn validate_required_files(artifact_dir: &Path, findings: &mut Vec<VerifyFinding>) {
    for file in [
        "activation.md",
        "catalog.json",
        "boundaries.json",
        "tool-interface.json",
        "references.index.json",
        "source-digest.txt",
    ] {
        let path = artifact_dir.join(file);
        if !path.is_file() {
            push_compile_finding(
                findings,
                "required_file_missing",
                "error",
                format!("required artifact file {file} is missing"),
                json!({ "path": path.display().to_string() }),
            );
        }
    }
}

fn validate_content_hashes(
    artifact_dir: &Path,
    manifest: &CompiledArtifactManifest,
    findings: &mut Vec<VerifyFinding>,
) {
    for (file, key) in REQUIRED_ARTIFACT_FILES {
        let path = artifact_dir.join(file);
        let Some(expected) = manifest.content_hashes.get(key) else {
            push_compile_finding(
                findings,
                "content_hash_missing",
                "error",
                format!("manifest content_hashes is missing {key}"),
                json!({ "file": file }),
            );
            continue;
        };
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let actual = digest_bytes_prefixed(&bytes);
        if actual != *expected {
            push_compile_finding(
                findings,
                "content_hash_mismatch",
                "error",
                format!("content hash mismatch for {file}"),
                json!({ "expected": expected, "actual": actual }),
            );
        }
    }
}

fn validate_sidecars(artifact_dir: &Path, skill_path: &Path, findings: &mut Vec<VerifyFinding>) {
    if let Some(catalog) = load_sidecar::<CatalogDoc>(&artifact_dir.join("catalog.json"), findings)
    {
        validate_sidecar_schema_version("catalog.json", catalog.schema_version, findings);
        validate_catalog(&catalog, findings);
    }
    if let Some(boundaries) =
        load_sidecar::<BoundariesDoc>(&artifact_dir.join("boundaries.json"), findings)
    {
        validate_sidecar_schema_version("boundaries.json", boundaries.schema_version, findings);
    }
    if let Some(interface) =
        load_sidecar::<ToolInterfaceDoc>(&artifact_dir.join("tool-interface.json"), findings)
    {
        validate_sidecar_schema_version("tool-interface.json", interface.schema_version, findings);
        validate_tool_interface(&interface, skill_path, artifact_dir, findings);
    }
    if let Some(references) =
        load_sidecar::<ReferencesDoc>(&artifact_dir.join("references.index.json"), findings)
    {
        validate_sidecar_schema_version(
            "references.index.json",
            references.schema_version,
            findings,
        );
        validate_references(&references, skill_path, artifact_dir, findings);
    }
}

fn load_sidecar<T: for<'de> Deserialize<'de>>(
    path: &Path,
    findings: &mut Vec<VerifyFinding>,
) -> Option<T> {
    let Ok(raw) = fs::read_to_string(path) else {
        return None;
    };
    match serde_json::from_str(&raw) {
        Ok(value) => Some(value),
        Err(err) => {
            push_compile_finding(
                findings,
                "sidecar_malformed",
                "error",
                "artifact sidecar JSON is malformed",
                json!({ "path": path.display().to_string(), "error": err.to_string() }),
            );
            None
        }
    }
}

fn validate_sidecar_schema_version(file: &str, actual: u32, findings: &mut Vec<VerifyFinding>) {
    if actual != COMPILE_SCHEMA_VERSION {
        push_compile_finding(
            findings,
            "sidecar_schema_mismatch",
            "error",
            format!("{file} has unsupported schema_version"),
            json!({ "expected": COMPILE_SCHEMA_VERSION, "actual": actual }),
        );
    }
}

fn validate_catalog(catalog: &CatalogDoc, findings: &mut Vec<VerifyFinding>) {
    let roles = [
        "activation",
        "boundary",
        "tool-interface",
        "reference-index",
        "metadata",
    ];
    let mut section_ids = BTreeSet::new();
    for section in &catalog.sections {
        if !section_ids.insert(section.id.clone()) {
            push_compile_finding(
                findings,
                "catalog_duplicate_section",
                "error",
                "catalog section ids must be unique",
                json!({"id": section.id.as_str()}),
            );
        }
        if !roles.contains(&section.role.as_str()) {
            push_compile_finding(
                findings,
                "catalog_role_invalid",
                "error",
                "catalog section role is unsupported",
                json!({"role": section.role.as_str()}),
            );
        }
    }
    for sidecar in &catalog.sidecars {
        if !sidecar.name.ends_with(".json") {
            push_compile_finding(
                findings,
                "catalog_sidecar_invalid",
                "error",
                "catalog sidecar names must be JSON files",
                json!({"name": sidecar.name.as_str()}),
            );
        }
    }
}

fn validate_tool_interface(
    interface: &ToolInterfaceDoc,
    skill_path: &Path,
    artifact_dir: &Path,
    findings: &mut Vec<VerifyFinding>,
) {
    for tool in &interface.allowed_tools {
        if !["agent-tool", "script", "mcp", "external-command"].contains(&tool.kind.as_str()) {
            push_compile_finding(
                findings,
                "tool_kind_invalid",
                "error",
                "tool-interface allowed tool kind is unsupported",
                json!({"kind": tool.kind.as_str()}),
            );
        }
    }
    for script in &interface.script_entrypoints {
        if !["low", "medium", "high", "blocked"].contains(&script.risk.as_str()) {
            push_compile_finding(
                findings,
                "script_risk_invalid",
                "error",
                "script risk is unsupported",
                json!({"risk": script.risk.as_str()}),
            );
        }
        validate_confined_path(
            &script.path,
            skill_path,
            artifact_dir,
            "script_path_escape",
            findings,
        );
    }
}

fn validate_references(
    references: &ReferencesDoc,
    skill_path: &Path,
    artifact_dir: &Path,
    findings: &mut Vec<VerifyFinding>,
) {
    for reference in &references.references {
        if !["always", "on-demand", "agent-profile"].contains(&reference.load_condition.as_str()) {
            push_compile_finding(
                findings,
                "reference_load_condition_invalid",
                "error",
                "reference load_condition is unsupported",
                json!({"load_condition": reference.load_condition.as_str()}),
            );
        }
        validate_confined_path(
            &reference.path,
            skill_path,
            artifact_dir,
            "reference_path_escape",
            findings,
        );
    }
}

fn validate_confined_path(
    raw: &str,
    skill_path: &Path,
    artifact_dir: &Path,
    id: &str,
    findings: &mut Vec<VerifyFinding>,
) {
    let path = Path::new(raw);
    if raw.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        push_compile_finding(
            findings,
            id,
            "error",
            "indexed path escapes its allowed root",
            json!({"path": raw}),
        );
        return;
    }
    for root in [skill_path, artifact_dir] {
        let candidate = root.join(path);
        if !candidate.exists() {
            continue;
        }
        match (candidate.canonicalize(), root.canonicalize()) {
            (Ok(candidate), Ok(root)) if candidate.starts_with(&root) => return,
            _ => {
                push_compile_finding(
                    findings,
                    id,
                    "error",
                    "indexed path canonicalizes outside its allowed root",
                    json!({"path": raw}),
                );
                return;
            }
        }
    }
}

fn validate_activation_text(path: &Path, findings: &mut Vec<VerifyFinding>) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    if raw.trim().is_empty() {
        push_compile_finding(
            findings,
            "activation_empty",
            "error",
            "activation.md must not be empty",
            json!({ "path": path.display().to_string() }),
        );
    }
    let lower = raw.to_ascii_lowercase();
    if lower.contains("rm -rf /") || lower.contains("curl | sh") {
        push_compile_finding(
            findings,
            "generated_activation_safety_blocked",
            "error",
            "generated activation contains a blocked unsafe command pattern",
            json!({ "path": path.display().to_string() }),
        );
    }
}

fn validate_source_digest(
    ctx: &AppContext,
    skill: &str,
    artifact_dir: &Path,
    manifest: &CompiledArtifactManifest,
    findings: &mut Vec<VerifyFinding>,
) -> std::result::Result<bool, CommandFailure> {
    let source_digest_path = artifact_dir.join("source-digest.txt");
    let sidecar_digest = fs::read_to_string(&source_digest_path)
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    if sidecar_digest != manifest.source_digest {
        push_compile_finding(
            findings,
            "source_digest_sidecar_mismatch",
            "error",
            "source-digest.txt does not match manifest.source_digest",
            json!({"expected": manifest.source_digest.as_str(), "actual": sidecar_digest}),
        );
    }
    let current = source_digest_info(ctx, skill, &manifest.agent, &manifest.profile)?;
    if current.digest != manifest.source_digest {
        push_compile_finding(
            findings,
            "source_digest_stale",
            "error",
            "compiled artifact source digest is stale",
            json!({"expected": manifest.source_digest.as_str(), "actual": current.digest}),
        );
        return Ok(true);
    }
    Ok(false)
}

fn validate_gate_status(manifest: &CompiledArtifactManifest, findings: &mut Vec<VerifyFinding>) {
    for (name, value) in [
        ("lint", manifest.gates.lint),
        ("safety", manifest.gates.safety),
        ("dependency", manifest.gates.dependency),
        ("eval", manifest.gates.eval),
    ] {
        if !value.allows_valid() {
            push_compile_finding(
                findings,
                "gate_blocks_valid_artifact",
                "blocked",
                format!("{name} gate is {}", value.as_str()),
                json!({ "gate": name, "status": value.as_str() }),
            );
        }
    }
    if manifest.status != ArtifactStatus::Valid {
        push_compile_finding(
            findings,
            "artifact_status_not_valid",
            "blocked",
            "artifact is not promoted to valid status",
            json!({ "status": manifest.status.as_str() }),
        );
    }
}
