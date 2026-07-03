use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{SkillsetAddArgs, SkillsetCreateArgs, SkillsetMemberArgs, SkillsetShowArgs};
use crate::envelope::Meta;
use crate::fs_util::write_atomic;
use crate::state::AppContext;
use crate::state_model::REGISTRY_SCHEMA_VERSION;
use crate::types::ErrorCode;

use super::helpers::{
    commit_registry_state, map_arg, map_io, map_lock, validate_non_empty, validate_skill_name,
};
use super::{App, CommandFailure, build_skill_read_model};

pub(crate) const SKILLSETS_REL: &str = "state/registry/skillsets.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SkillsetsFile {
    pub(crate) schema_version: u32,
    pub(crate) skillsets: Vec<SkillsetRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SkillsetRecord {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    pub(crate) members: Vec<SkillsetMemberRecord>,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct SkillsetMemberRecord {
    pub(crate) skill_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    pub(crate) required: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillsetPackageSource {
    pub id: String,
    pub description: Option<String>,
    pub members: Vec<SkillsetPackageMember>,
}

#[derive(Debug, Clone)]
pub(crate) struct SkillsetPackageMember {
    pub skill_id: String,
    pub role: Option<String>,
    pub required: bool,
}

impl SkillsetsFile {
    fn empty() -> Self {
        Self {
            schema_version: REGISTRY_SCHEMA_VERSION,
            skillsets: Vec::new(),
        }
    }

    pub(crate) fn normalize(&mut self) {
        self.skillsets.sort_by(|left, right| left.id.cmp(&right.id));
        for skillset in &mut self.skillsets {
            skillset
                .members
                .sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
        }
    }

    pub(crate) fn find(&self, name: &str) -> Option<&SkillsetRecord> {
        self.skillsets.iter().find(|skillset| skillset.id == name)
    }

    pub(crate) fn find_mut(&mut self, name: &str) -> Option<&mut SkillsetRecord> {
        self.skillsets
            .iter_mut()
            .find(|skillset| skillset.id == name)
    }
}

impl App {
    pub fn cmd_skillset_create(
        &self,
        args: &SkillsetCreateArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let description = normalize_optional_text("description", args.description.as_deref())?;

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut file = load_skillsets(&self.ctx)?;
        if file.find(&args.name).is_some() {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!("skillset '{}' already exists", args.name),
            ));
        }

        let now = Utc::now();
        file.skillsets.push(SkillsetRecord {
            id: args.name.clone(),
            description,
            members: Vec::new(),
            created_at: now,
            updated_at: now,
        });
        save_skillsets(&self.ctx, &mut file)?;
        let commit = commit_registry_state(&self.ctx, &format!("skillset({}): create", args.name))?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("skillset '{}' missing after create", args.name),
            )
        })?;
        Ok((
            json!({
                "skillset": render_skillset(skillset, None),
                "path": paths.registry_dir.join("skillsets.json"),
                "commit": commit,
                "next_actions": [
                    format!("loom skillset add {} <skill>", args.name),
                    format!("loom skillset lint {}", args.name)
                ],
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_skillset_add(
        &self,
        args: &SkillsetAddArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        validate_skill_name(&args.skill).map_err(map_arg)?;
        let role = normalize_optional_text("role", args.role.as_deref())?;
        let required = !args.optional;

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;
        ensure_inventory_skill_exists(&self.ctx, &args.skill)?;

        let mut file = load_skillsets(&self.ctx)?;
        let skillset = file.find_mut(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        if skillset
            .members
            .iter()
            .any(|member| member.skill_id == args.skill)
        {
            return Err(CommandFailure::new(
                ErrorCode::ArgInvalid,
                format!(
                    "skill '{}' is already a member of skillset '{}'",
                    args.skill, args.name
                ),
            ));
        }

        skillset.members.push(SkillsetMemberRecord {
            skill_id: args.skill.clone(),
            role,
            required,
        });
        skillset.updated_at = Utc::now();
        save_skillsets(&self.ctx, &mut file)?;
        let commit = commit_registry_state(
            &self.ctx,
            &format!("skillset({}): add {}", args.name, args.skill),
        )?;
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("skillset '{}' missing after add", args.name),
            )
        })?;
        Ok((
            json!({
                "skillset": render_skillset(skillset, Some(&inventory)),
                "commit": commit,
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_skillset_remove(
        &self,
        args: &SkillsetMemberArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        validate_skill_name(&args.skill).map_err(map_arg)?;

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        self.ensure_registry_layout()?;
        let mut file = load_skillsets(&self.ctx)?;
        let skillset = file.find_mut(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let before = skillset.members.len();
        skillset
            .members
            .retain(|member| member.skill_id != args.skill);
        if skillset.members.len() == before {
            return Err(CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!(
                    "skill '{}' is not a member of skillset '{}'",
                    args.skill, args.name
                ),
            ));
        }
        skillset.updated_at = Utc::now();
        save_skillsets(&self.ctx, &mut file)?;
        let commit = commit_registry_state(
            &self.ctx,
            &format!("skillset({}): remove {}", args.name, args.skill),
        )?;
        let inventory = skill_inventory_by_id(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("skillset '{}' missing after remove", args.name),
            )
        })?;
        Ok((
            json!({
                "skillset": render_skillset(skillset, Some(&inventory)),
                "commit": commit,
            }),
            Meta::default(),
        ))
    }

    pub fn cmd_skillset_show(
        &self,
        args: &SkillsetShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let inventory = skill_inventory_by_id(&self.ctx)?;
        Ok((
            json!({ "skillset": render_skillset(skillset, Some(&inventory)) }),
            Meta::default(),
        ))
    }

    pub fn cmd_skillset_lint(
        &self,
        args: &SkillsetShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_skillset_id(&args.name)?;
        let file = load_skillsets(&self.ctx)?;
        let skillset = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("skillset '{}' not found", args.name),
            )
        })?;
        let inventory = skill_inventory_by_id(&self.ctx)?;
        Ok((lint_skillset(skillset, &inventory), Meta::default()))
    }
}

pub(crate) fn validate_skillset_id(name: &str) -> std::result::Result<(), CommandFailure> {
    validate_skill_name(name).map_err(map_arg)
}

fn normalize_optional_text(
    name: &str,
    value: Option<&str>,
) -> std::result::Result<Option<String>, CommandFailure> {
    let Some(value) = value else {
        return Ok(None);
    };
    let trimmed = value.trim();
    validate_non_empty(name, trimmed)?;
    Ok(Some(trimmed.to_string()))
}

fn skillsets_path(ctx: &AppContext) -> PathBuf {
    ctx.root.join(SKILLSETS_REL)
}

pub(crate) fn load_skillsets(
    ctx: &AppContext,
) -> std::result::Result<SkillsetsFile, CommandFailure> {
    let path = skillsets_path(ctx);
    if !path.exists() {
        return Ok(SkillsetsFile::empty());
    }
    let raw = fs::read_to_string(&path).map_err(map_io)?;
    parse_skillsets_file(&raw, &path.display().to_string())
}

pub(crate) fn parse_skillsets_file(
    raw: &str,
    label: &str,
) -> std::result::Result<SkillsetsFile, CommandFailure> {
    let file: SkillsetsFile = serde_json::from_str(raw).map_err(|err| {
        CommandFailure::new(
            ErrorCode::StateCorrupt,
            format!("failed to parse {}: {}", label, err),
        )
    })?;
    if file.schema_version != REGISTRY_SCHEMA_VERSION {
        return Err(CommandFailure::new(
            ErrorCode::SchemaMismatch,
            format!(
                "{} schema_version {} is not supported",
                label, file.schema_version
            ),
        ));
    }
    Ok(file)
}

pub(crate) fn load_skillset_package_source(
    ctx: &AppContext,
    name: &str,
) -> std::result::Result<SkillsetPackageSource, CommandFailure> {
    validate_skillset_id(name)?;
    let file = load_skillsets(ctx)?;
    let skillset = file.find(name).ok_or_else(|| {
        CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skillset '{}' not found", name),
        )
    })?;
    Ok(SkillsetPackageSource {
        id: skillset.id.clone(),
        description: skillset.description.clone(),
        members: skillset
            .members
            .iter()
            .map(|member| SkillsetPackageMember {
                skill_id: member.skill_id.clone(),
                role: member.role.clone(),
                required: member.required,
            })
            .collect(),
    })
}

pub(crate) fn save_skillsets(
    ctx: &AppContext,
    file: &mut SkillsetsFile,
) -> std::result::Result<(), CommandFailure> {
    file.normalize();
    let path = skillsets_path(ctx);
    let raw = serde_json::to_string_pretty(file).map_err(map_io)? + "\n";
    write_atomic(&path, &raw).map_err(map_io)
}

pub(crate) fn skill_inventory_by_id(
    ctx: &AppContext,
) -> std::result::Result<BTreeMap<String, Value>, CommandFailure> {
    let model = build_skill_read_model(ctx)
        .map_err(|err| CommandFailure::new(ErrorCode::InternalError, err.to_string()))?;
    let mut out = BTreeMap::new();
    for skill in model.skills {
        if let Some(skill_id) = skill["skill_id"].as_str() {
            out.insert(skill_id.to_string(), skill);
        }
    }
    Ok(out)
}

fn ensure_inventory_skill_exists(
    ctx: &AppContext,
    skill: &str,
) -> std::result::Result<(), CommandFailure> {
    let inventory = skill_inventory_by_id(ctx)?;
    if inventory.contains_key(skill) {
        return Ok(());
    }
    Err(CommandFailure::new(
        ErrorCode::SkillNotFound,
        format!("skill '{}' not found", skill),
    ))
}

pub(crate) fn render_skillset(
    skillset: &SkillsetRecord,
    inventory: Option<&BTreeMap<String, Value>>,
) -> Value {
    let mut missing = 0usize;
    let mut required = 0usize;
    let mut optional = 0usize;
    let members = skillset
        .members
        .iter()
        .map(|member| {
            if member.required {
                required += 1;
            } else {
                optional += 1;
            }
            let skill = inventory
                .and_then(|items| items.get(&member.skill_id))
                .cloned();
            if skill.is_none() {
                missing += 1;
            }
            json!({
                "skill_id": member.skill_id,
                "role": member.role,
                "required": member.required,
                "missing": skill.is_none(),
                "skill": skill,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "id": skillset.id,
        "description": skillset.description,
        "members": members,
        "summary": {
            "members": skillset.members.len(),
            "required": required,
            "optional": optional,
            "missing": missing,
        },
        "created_at": skillset.created_at,
        "updated_at": skillset.updated_at,
    })
}

pub(crate) fn lint_skillset(
    skillset: &SkillsetRecord,
    inventory: &BTreeMap<String, Value>,
) -> Value {
    let mut findings = Vec::new();
    let mut seen = BTreeSet::new();
    let mut duplicates = 0usize;
    let mut missing = 0usize;
    let mut required = 0usize;
    let mut optional = 0usize;

    if skillset.members.is_empty() {
        findings.push(json!({
            "id": "skillset_empty",
            "severity": "warning",
            "message": "skillset has no members",
        }));
    }

    for member in &skillset.members {
        if member.required {
            required += 1;
        } else {
            optional += 1;
        }
        if !seen.insert(member.skill_id.clone()) {
            duplicates += 1;
            findings.push(json!({
                "id": "duplicate_member",
                "severity": "error",
                "skill_id": member.skill_id,
                "message": format!("skill '{}' appears more than once", member.skill_id),
            }));
        }
        if !inventory.contains_key(&member.skill_id) {
            missing += 1;
            findings.push(json!({
                "id": "member_missing",
                "severity": if member.required { "error" } else { "warning" },
                "skill_id": member.skill_id,
                "message": format!("member skill '{}' is missing", member.skill_id),
            }));
        }
    }

    let valid = findings
        .iter()
        .all(|finding| finding["severity"].as_str() != Some("error"));

    json!({
        "skillset": skillset.id,
        "valid": valid,
        "summary": {
            "members": skillset.members.len(),
            "required": required,
            "optional": optional,
            "missing": missing,
            "duplicates": duplicates,
        },
        "findings": findings,
    })
}
