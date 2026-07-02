use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use walkdir::WalkDir;

use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;

use super::super::helpers::map_io;
use super::super::skill_deps::skill_dependency_report;
use super::super::skill_safety::evaluate_skill_safety;
use super::super::{CommandFailure, SkillLintMode, lint_skill_source};
use super::model::{
    BoundariesDoc, COMPILE_SCHEMA_VERSION, COMPILER_VERSION, CatalogDoc, CatalogSection,
    CatalogSidecar, CompileGateStatus, CompileTokenEstimate, GatePlan, GateValue,
    MIN_SOURCE_TOKENS, PlannedArtifact, ReferenceRecord, ReferencesDoc, ScriptRecord,
    SourceDigestInfo, SourceDigestInput, ToolInterfaceDoc, ToolRecord,
};
use super::util::{
    compiled_skill_root, digest_bytes_prefixed, estimate_tokens, frontmatter_value,
    push_unique_limited, sanitize_artifact_part, slash_path, stable_json, update_digest_field,
};

pub(super) fn source_digest_info(
    ctx: &AppContext,
    skill: &str,
    agent: &str,
    profile: &str,
) -> std::result::Result<SourceDigestInfo, CommandFailure> {
    let inputs = collect_source_inputs(&ctx.skill_path(skill))?;
    let mut hasher = Sha256::new();
    for part in [
        "loom-compiled-source-v1",
        COMPILER_VERSION,
        skill,
        agent,
        profile,
    ] {
        update_digest_field(&mut hasher, part);
    }
    for input in &inputs {
        update_digest_field(&mut hasher, &input.path);
        update_digest_field(&mut hasher, &input.kind);
        update_digest_field(&mut hasher, &input.size.to_string());
        update_digest_field(&mut hasher, &input.sha256);
        if let Some(executable) = input.executable {
            update_digest_field(&mut hasher, if executable { "x" } else { "-" });
        }
        if let Some(target) = &input.symlink_target {
            update_digest_field(&mut hasher, target);
        }
    }
    Ok(SourceDigestInfo {
        digest: format!("sha256:{}", to_hex(&hasher.finalize())),
        inputs,
    })
}

pub(super) fn planned_artifact(
    ctx: &AppContext,
    skill: &str,
    agent: &str,
    profile: &str,
    source: &SourceDigestInfo,
) -> std::result::Result<PlannedArtifact, CommandFailure> {
    let skill_path = ctx.skill_path(skill);
    let skill_md = fs::read_to_string(skill_path.join("SKILL.md")).map_err(map_io)?;
    let source_tokens = estimate_tokens(&skill_md);
    let artifact_id = artifact_id_for(skill, agent, profile, &source.digest);
    let artifact_root = compiled_skill_root(ctx, skill).join(&artifact_id);

    let boundaries = build_boundaries(&skill_md);
    let references = build_references(&skill_path, source)?;
    let tool_interface = build_tool_interface(&skill_path, &skill_md, source);
    let activation_md = build_activation(skill, agent, profile, &skill_md, &boundaries);
    let boundaries_json = stable_json(&boundaries)?;
    let references_json = stable_json(&references)?;
    let tool_interface_json = stable_json(&tool_interface)?;

    let mut content_hashes = BTreeMap::new();
    content_hashes.insert(
        "activation_md".to_string(),
        digest_bytes_prefixed(activation_md.as_bytes()),
    );
    content_hashes.insert(
        "boundaries_json".to_string(),
        digest_bytes_prefixed(boundaries_json.as_bytes()),
    );
    content_hashes.insert(
        "references_index_json".to_string(),
        digest_bytes_prefixed(references_json.as_bytes()),
    );
    content_hashes.insert(
        "tool_interface_json".to_string(),
        digest_bytes_prefixed(tool_interface_json.as_bytes()),
    );
    let catalog = build_catalog(&content_hashes);
    let catalog_json = stable_json(&catalog)?;
    content_hashes.insert(
        "catalog_json".to_string(),
        digest_bytes_prefixed(catalog_json.as_bytes()),
    );

    let compiled_runtime_total = estimate_tokens(&activation_md)
        + estimate_tokens(&boundaries_json)
        + estimate_tokens(&references_json)
        + estimate_tokens(&tool_interface_json)
        + estimate_tokens(&catalog_json);
    let token_estimate = CompileTokenEstimate {
        source_skill_md: source_tokens,
        activation_md: estimate_tokens(&activation_md),
        compiled_runtime_total,
    };
    let no_op_reason = no_op_reason(source_tokens, compiled_runtime_total);
    let paths = artifact_paths(&artifact_root);
    Ok(PlannedArtifact {
        artifact_id,
        artifact_root,
        paths,
        content_hashes,
        token_estimate,
        no_op: no_op_reason.is_some(),
        no_op_reason,
        content: json!({
            "activation.md": activation_md,
            "catalog.json": serde_json::from_str::<Value>(&catalog_json).map_err(map_io)?,
            "boundaries.json": serde_json::from_str::<Value>(&boundaries_json).map_err(map_io)?,
            "tool-interface.json": serde_json::from_str::<Value>(&tool_interface_json).map_err(map_io)?,
            "references.index.json": serde_json::from_str::<Value>(&references_json).map_err(map_io)?,
            "source-digest.txt": format!("{}\n", source.digest),
        }),
    })
}

pub(super) fn compile_gates(ctx: &AppContext, skill: &str, agent: &str) -> GatePlan {
    let skill_path = ctx.skill_path(skill);
    let lint = lint_skill_source_for_gate(&skill_path, skill);
    let safety = match evaluate_skill_safety(ctx, skill, "compile", false) {
        Ok(report) if report.activation_allowed => gate_detail(GateValue::Pass, report.decision),
        Ok(report) => gate_detail(GateValue::Blocked, report.decision),
        Err(err) => gate_detail(GateValue::Blocked, err.message),
    };
    let dependency = match skill_dependency_report(ctx, skill, Some(agent), None) {
        Ok(report) if report.ready => gate_detail(GateValue::Pass, report.status),
        Ok(report) if report.status == "unknown" => gate_detail(GateValue::Missing, report.status),
        Ok(report) => gate_detail(GateValue::Blocked, report.status),
        Err(err) => gate_detail(GateValue::Missing, err.message),
    };
    let eval = gate_detail(
        GateValue::Missing,
        "eval evidence is deferred until a reviewed eval artifact exists",
    );
    GatePlan {
        manifest: CompileGateStatus {
            lint: lint.0,
            safety: safety.0,
            dependency: dependency.0,
            eval: eval.0,
        },
        details: json!({
            "lint": lint.1,
            "safety": safety.1,
            "dependency": dependency.1,
            "eval": eval.1,
        }),
    }
}

fn collect_source_inputs(
    skill_path: &Path,
) -> std::result::Result<Vec<SourceDigestInput>, CommandFailure> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(skill_path).follow_links(false) {
        let entry = entry.map_err(map_io)?;
        if entry.path() != skill_path {
            paths.push(entry.path().to_path_buf());
        }
    }
    paths.sort_by_key(|path| slash_path(path));

    let mut inputs = Vec::new();
    for path in paths {
        let metadata = fs::symlink_metadata(&path).map_err(map_io)?;
        let rel = path
            .strip_prefix(skill_path)
            .map_err(map_io)
            .map(slash_path)?;
        if metadata.is_dir() {
            continue;
        }
        if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(map_io)?;
            inputs.push(SourceDigestInput {
                path: rel,
                kind: "symlink".to_string(),
                size: 0,
                sha256: digest_bytes_prefixed(target.to_string_lossy().as_bytes()),
                executable: None,
                symlink_target: Some(target.display().to_string()),
            });
            continue;
        }
        if metadata.is_file() {
            let bytes = fs::read(&path).map_err(map_io)?;
            inputs.push(SourceDigestInput {
                path: rel,
                kind: "file".to_string(),
                size: metadata.len(),
                sha256: digest_bytes_prefixed(&bytes),
                executable: Some(source_file_is_executable(&metadata)),
                symlink_target: None,
            });
        }
    }
    Ok(inputs)
}

#[cfg(unix)]
fn source_file_is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn source_file_is_executable(_metadata: &fs::Metadata) -> bool {
    false
}

fn artifact_id_for(skill: &str, agent: &str, profile: &str, digest: &str) -> String {
    let source_prefix = digest
        .strip_prefix("sha256:")
        .unwrap_or(digest)
        .chars()
        .take(16)
        .collect::<String>();
    format!(
        "compiled_{}_{}_{}_{}",
        sanitize_artifact_part(skill),
        sanitize_artifact_part(agent),
        sanitize_artifact_part(profile),
        source_prefix
    )
}

fn no_op_reason(source_tokens: usize, compiled_runtime_total: usize) -> Option<String> {
    if source_tokens < MIN_SOURCE_TOKENS {
        Some(format!(
            "source estimate {} is below compile threshold {}",
            source_tokens, MIN_SOURCE_TOKENS
        ))
    } else if compiled_runtime_total.saturating_mul(100) > source_tokens.saturating_mul(90) {
        Some("planned runtime would not reduce required context by at least 10%".to_string())
    } else {
        None
    }
}

fn artifact_paths(artifact_root: &Path) -> BTreeMap<String, String> {
    let mut paths = BTreeMap::new();
    for name in [
        "manifest.json",
        "activation.md",
        "catalog.json",
        "boundaries.json",
        "tool-interface.json",
        "references.index.json",
        "source-digest.txt",
    ] {
        paths.insert(
            name.to_string(),
            artifact_root.join(name).display().to_string(),
        );
    }
    paths
}

fn build_activation(
    skill: &str,
    agent: &str,
    profile: &str,
    skill_md: &str,
    boundaries: &BoundariesDoc,
) -> String {
    let description = frontmatter_value(skill_md, "description").unwrap_or("not declared");
    let headings = skill_md
        .lines()
        .filter(|line| line.trim_start().starts_with('#'))
        .take(16)
        .map(|line| line.trim().to_string())
        .collect::<Vec<_>>();
    let mut body = String::new();
    body.push_str(&format!("# Compiled Activation: {skill}\n\n"));
    body.push_str(&format!("- Source skill: {skill}\n"));
    body.push_str(&format!("- Agent: {agent}\n"));
    body.push_str(&format!("- Profile: {profile}\n"));
    body.push_str(&format!("- Compiler: {COMPILER_VERSION}\n\n"));
    body.push_str("## Description\n");
    body.push_str(description);
    body.push_str("\n\n## Trigger Boundaries\n");
    append_list(
        &mut body,
        &boundaries.triggers,
        "No explicit triggers extracted.",
    );
    body.push_str("\n## Non-Triggers And Safety\n");
    append_list(
        &mut body,
        &boundaries.non_triggers,
        "No explicit non-triggers extracted.",
    );
    body.push_str("\n## Source Outline\n");
    append_list(&mut body, &headings, "No headings extracted.");
    body.push_str("\n## Deferred Operations\n");
    append_list(&mut body, &boundaries.deferred_operations, "None.");
    body
}

fn append_list(body: &mut String, items: &[String], empty: &str) {
    if items.is_empty() {
        body.push_str(empty);
        body.push('\n');
        return;
    }
    for item in items {
        body.push_str("- ");
        body.push_str(item);
        body.push('\n');
    }
}

fn build_boundaries(skill_md: &str) -> BoundariesDoc {
    let mut triggers = Vec::new();
    let mut non_triggers = Vec::new();
    for line in skill_md
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let lower = line.to_ascii_lowercase();
        if lower.contains("use when")
            || lower.contains("trigger")
            || lower.contains("should be used")
        {
            push_unique_limited(&mut triggers, line, 12);
        }
        if lower.contains("do not")
            || lower.contains("non-goal")
            || lower.contains("must not")
            || lower.contains("never")
        {
            push_unique_limited(&mut non_triggers, line, 12);
        }
    }
    BoundariesDoc {
        schema_version: COMPILE_SCHEMA_VERSION,
        triggers,
        non_triggers,
        deferred_operations: vec![
            "artifact writes".to_string(),
            "compiled activation".to_string(),
            "remote LLM summarization".to_string(),
            "eval-backed promotion".to_string(),
        ],
        required_handoff_fields: Vec::new(),
    }
}

fn build_tool_interface(
    skill_path: &Path,
    skill_md: &str,
    source: &SourceDigestInfo,
) -> ToolInterfaceDoc {
    let mut allowed_tools = Vec::new();
    if let Some(raw) = frontmatter_value(skill_md, "allowed-tools") {
        for name in raw
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            allowed_tools.push(ToolRecord {
                name: name.to_string(),
                kind: "agent-tool".to_string(),
            });
        }
    }
    let mut script_entrypoints = source
        .inputs
        .iter()
        .filter(|input| input.kind == "file" && input.path.starts_with("scripts/"))
        .map(|input| ScriptRecord {
            path: input.path.clone(),
            usage: "on-demand".to_string(),
            risk: script_risk(skill_path, &input.path),
        })
        .collect::<Vec<_>>();
    script_entrypoints.sort_by(|a, b| a.path.cmp(&b.path));
    ToolInterfaceDoc {
        schema_version: COMPILE_SCHEMA_VERSION,
        allowed_tools,
        script_entrypoints,
    }
}

fn build_references(
    skill_path: &Path,
    source: &SourceDigestInfo,
) -> std::result::Result<ReferencesDoc, CommandFailure> {
    let mut references = Vec::new();
    for input in &source.inputs {
        if input.kind != "file"
            || !(input.path.starts_with("references/") || input.path.starts_with("assets/"))
        {
            continue;
        }
        let path = skill_path.join(&input.path);
        references.push(ReferenceRecord {
            path: input.path.clone(),
            role: "metadata".to_string(),
            load_condition: "on-demand".to_string(),
            content_hash: if path.is_file() {
                digest_bytes_prefixed(&fs::read(&path).map_err(map_io)?)
            } else {
                input.sha256.clone()
            },
        });
    }
    references.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ReferencesDoc {
        schema_version: COMPILE_SCHEMA_VERSION,
        references,
    })
}

fn build_catalog(content_hashes: &BTreeMap<String, String>) -> CatalogDoc {
    CatalogDoc {
        schema_version: COMPILE_SCHEMA_VERSION,
        sections: vec![CatalogSection {
            id: "activation".to_string(),
            title: "Activation".to_string(),
            content_hash: content_hashes
                .get("activation_md")
                .cloned()
                .unwrap_or_default(),
            role: "activation".to_string(),
        }],
        sidecars: vec![
            catalog_sidecar("boundaries.json", "boundaries", "boundaries_json"),
            catalog_sidecar(
                "tool-interface.json",
                "tool-interface",
                "tool_interface_json",
            ),
            catalog_sidecar(
                "references.index.json",
                "references-index",
                "references_index_json",
            ),
        ]
        .into_iter()
        .map(|mut sidecar| {
            sidecar.content_hash = content_hashes
                .get(sidecar_hash_key(&sidecar.name))
                .cloned()
                .unwrap_or_default();
            sidecar
        })
        .collect(),
    }
}

fn catalog_sidecar(name: &str, schema: &str, hash_key: &str) -> CatalogSidecar {
    CatalogSidecar {
        name: name.to_string(),
        schema: schema.to_string(),
        content_hash: hash_key.to_string(),
        required: true,
    }
}

fn sidecar_hash_key(name: &str) -> &'static str {
    match name {
        "boundaries.json" => "boundaries_json",
        "tool-interface.json" => "tool_interface_json",
        "references.index.json" => "references_index_json",
        _ => "catalog_json",
    }
}

fn lint_skill_source_for_gate(skill_path: &Path, skill: &str) -> (GateValue, Value) {
    let report = lint_skill_source(skill_path, skill, SkillLintMode::Strict);
    let gate = if report.summary.error_count > 0 {
        GateValue::Fail
    } else if report.summary.warning_count > 0 {
        GateValue::Warning
    } else {
        GateValue::Pass
    };
    (
        gate,
        json!({
            "status": gate.as_str(),
            "errors": report.summary.error_count,
            "warnings": report.summary.warning_count,
        }),
    )
}

fn gate_detail(status: GateValue, reason: impl Into<String>) -> (GateValue, Value) {
    (
        status,
        json!({
            "status": status.as_str(),
            "reason": reason.into(),
        }),
    )
}

fn script_risk(skill_path: &Path, rel: &str) -> String {
    let raw = fs::read_to_string(skill_path.join(rel)).unwrap_or_default();
    let lower = raw.to_ascii_lowercase();
    if lower.contains("rm -rf") || lower.contains("curl | sh") {
        "blocked"
    } else if lower.contains("curl") || lower.contains("http") {
        "high"
    } else {
        "medium"
    }
    .to_string()
}
