mod model;
mod store;
mod validate;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{
    WorkflowCommand, WorkflowCreateArgs, WorkflowPlanArgs, WorkflowPreflightArgs, WorkflowRunArgs,
    WorkflowShowArgs,
};
use crate::envelope::Meta;
use crate::gitops;
use crate::next_action_trace::observe_next_actions;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use crate::state_model::{RegistrySnapshot, RegistryStatePaths};
use crate::types::ErrorCode;

use super::helpers::{
    agent_kind_as_str, commit_registry_state, map_arg, map_git, map_io, map_lock,
    map_registry_state, shell_arg, slugify, validate_skill_name,
};
use super::provenance::skill_tree_digest;
use super::skill_policy::{SkillPolicyReport, evaluate_skill_policy};
use super::{App, CommandFailure};
use model::{
    PLAN_PROTOCOL_VERSION, StoredWorkflowPlan, WORKFLOW_PLAN_SCHEMA, WorkflowEdge, WorkflowInput,
    WorkflowNode, WorkflowRecord,
};
use store::{
    find_workflow, load_workflow_plans, load_workflows, save_workflow_plan, save_workflows,
};
use validate::{
    validate_plan_id, validate_workflow_definition, validate_workflow_id, validation_error,
    workflow_node,
};

impl App {
    pub fn cmd_workflow(
        &self,
        command: &WorkflowCommand,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        match command {
            WorkflowCommand::Create(args) => self.cmd_workflow_create(args),
            WorkflowCommand::Show(args) => self.cmd_workflow_show(args),
            WorkflowCommand::Plan(args) => self.cmd_workflow_plan(args),
            WorkflowCommand::Preflight(args) => self.cmd_workflow_preflight(args),
            WorkflowCommand::Run(args) => self.cmd_workflow_run(args),
        }
    }

    fn cmd_workflow_create(
        &self,
        args: &WorkflowCreateArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_workflow_id(&args.name)?;
        let has_file = args.file.is_some();
        let has_skillset = args.from_skillset.is_some();
        if has_file == has_skillset {
            return Err(validation_error(
                "WORKFLOW_SOURCE_INVALID",
                "provide exactly one of --file or --from-skillset",
            ));
        }
        if has_skillset && !args.dry_run {
            return Err(validation_error(
                "SKILLSET_WORKFLOW_DRY_RUN_ONLY",
                "--from-skillset is preview-only until workflow apply gates are implemented",
            ));
        }

        let now = Utc::now();
        let workflow = if let Some(path) = args.file.as_ref() {
            workflow_from_file(&args.name, path, now)?
        } else {
            workflow_from_skillset(&self.ctx, &args.name, args.from_skillset.as_deref(), now)?
        };
        let order = validate_workflow_definition(&workflow)?;

        if args.dry_run {
            return Ok((
                json!({
                    "workflow": render_workflow(&workflow, &order),
                    "dry_run": true,
                    "writes": [],
                    "next_actions": observe_next_actions(
                        "workflow.create.plan",
                        [format!("loom workflow create {} --file <workflow.json>", shell_arg(&args.name))],
                    ),
                }),
                Meta::default(),
            ));
        }

        let _workspace = self.ctx.lock_workspace().map_err(map_lock)?;
        self.ensure_write_repo_ready()?;
        let paths = self.ensure_registry_layout()?;
        let mut file = load_workflows(&self.ctx)?;
        if file.find(&args.name).is_some() {
            return Err(validation_error(
                "WORKFLOW_EXISTS",
                format!("workflow '{}' already exists", args.name),
            ));
        }
        file.workflows.push(workflow);
        save_workflows(&self.ctx, &mut file)?;
        let commit = commit_registry_state(&self.ctx, &format!("workflow({}): create", args.name))?;
        let workflow = file.find(&args.name).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::InternalError,
                format!("workflow '{}' missing after create", args.name),
            )
        })?;
        let order = validate_workflow_definition(workflow)?;
        Ok((
            json!({
                "workflow": render_workflow(workflow, &order),
                "path": paths.registry_dir.join("workflows.json"),
                "commit": commit,
                "next_actions": observe_next_actions(
                    "workflow.create.applied",
                    [
                        format!("loom workflow plan {} --agent <agent> --workspace <path>", shell_arg(&args.name)),
                        "loom workflow preflight <plan-id>".to_string()
                    ],
                ),
            }),
            Meta::default(),
        ))
    }

    fn cmd_workflow_show(
        &self,
        args: &WorkflowShowArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_workflow_id(&args.name)?;
        let file = load_workflows(&self.ctx)?;
        let workflow = find_workflow(&file, &args.name)?;
        let order = validate_workflow_definition(workflow)?;
        Ok((json!(render_workflow(workflow, &order)), Meta::default()))
    }

    fn cmd_workflow_plan(
        &self,
        args: &WorkflowPlanArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_workflow_id(&args.workflow)?;
        let file = load_workflows(&self.ctx)?;
        let workflow = find_workflow(&file, &args.workflow)?.clone();
        let order = validate_workflow_definition(&workflow)?;
        let paths = RegistryStatePaths::from_app_context(&self.ctx);
        let snapshot = paths
            .maybe_load_snapshot()
            .map_err(map_registry_state)?
            .ok_or_else(|| {
                CommandFailure::new(
                    ErrorCode::StateNotInitialized,
                    "registry state must be initialized before planning workflows",
                )
            })?;
        let trust = paths.load_trust().map_err(map_registry_state)?;
        let agent = agent_kind_as_str(args.agent).to_string();
        let workspace = absolute_path(&args.workspace)?;
        let registry_head = gitops::head(&self.ctx).map_err(map_git)?;
        let root = canonical_root(&self.ctx.root)?;

        let mut skill_digests = BTreeMap::new();
        let mut node_payloads = Vec::new();
        let mut activation_steps = Vec::new();
        let mut required_approvals = BTreeSet::new();
        let mut risks = Vec::new();

        for node_id in &order {
            let node = workflow_node(&workflow, node_id)?;
            validate_skill_name(&node.skill_id).map_err(map_arg)?;
            let skill_path = self.ctx.skill_path(&node.skill_id);
            if !skill_path.is_dir() {
                return Err(CommandFailure::new(
                    ErrorCode::SkillNotFound,
                    format!(
                        "skill '{}' not found for workflow node '{}'",
                        node.skill_id, node.id
                    ),
                ));
            }
            if let Some(trust_record) = trust
                .skills
                .iter()
                .find(|record| record.skill_id == node.skill_id)
                && (trust_record.quarantined || trust_record.trust == "blocked")
            {
                let mut failure = CommandFailure::new(
                    ErrorCode::PolicyBlocked,
                    format!("skill '{}' is blocked by trust policy", node.skill_id),
                );
                failure.details = json!({
                    "workflow": workflow.workflow_id,
                    "node": node.id,
                    "skill": node.skill_id,
                    "trust": trust_record.trust,
                    "quarantined": trust_record.quarantined,
                    "reason": trust_record.reason,
                });
                return Err(failure);
            }

            let digest = skill_tree_digest(&skill_path).map_err(map_io)?;
            skill_digests.insert(node.skill_id.clone(), digest.clone());
            let active = skill_active_for_workspace(&snapshot, &node.skill_id, &agent, &workspace);
            if !active {
                activation_steps.push(json!({
                    "node_id": node.id,
                    "skill": node.skill_id,
                    "status": "required",
                    "command": format!(
                        "loom skill activate {} --agent {} --workspace {} --scope project --dry-run",
                        shell_arg(&node.skill_id),
                        shell_arg(&agent),
                        shell_arg(&workspace)
                    ),
                }));
            }

            let policy = evaluate_skill_policy(&self.ctx, &node.skill_id, "safe-capture")?;
            add_policy_approvals(&policy, &mut required_approvals);
            add_policy_risks(&node.id, &policy, &mut risks);
            if node.mutates_workspace && workflow.policy.approval_required_for_mutations {
                required_approvals.insert(approval_token(&node.id));
            }
            if workflow
                .policy
                .requires_human_approval_before
                .iter()
                .any(|approval_node| approval_node == &node.id)
            {
                required_approvals.insert(approval_token(&node.id));
            }

            node_payloads.push(json!({
                "id": node.id,
                "skill": node.skill_id,
                "kind": node.kind,
                "requires": node.requires,
                "outputs": node.outputs,
                "mutates_workspace": node.mutates_workspace,
                "source_digest": digest,
                "active_for_agent": active,
            }));
        }

        let required_approvals = required_approvals.into_iter().collect::<Vec<_>>();
        let workflow_digest = digest_workflow(&workflow)?;
        let ready = activation_steps.is_empty() && required_approvals.is_empty();
        let plan_id = format!("workflow_plan_{}", Uuid::new_v4().simple());
        let checks = vec![
            json!({"id": "workflow_dag", "status": "pass", "nodes": order.len()}),
            json!({"id": "registry_head", "status": "pass", "head": registry_head}),
            json!({"id": "autonomous_execution", "status": "blocked", "reason": "workflow execution is deferred"}),
        ];
        let payload = json!({
            "protocol_version": PLAN_PROTOCOL_VERSION,
            "schema_version": WORKFLOW_PLAN_SCHEMA,
            "plan_id": plan_id,
            "operation": "workflow",
            "workflow_id": workflow.workflow_id,
            "description": workflow.description,
            "agent": agent,
            "workspace": workspace,
            "ordered_node_ids": order,
            "nodes": node_payloads,
            "edges": workflow.edges,
            "external_inputs": workflow.external_inputs,
            "policy": workflow.policy,
            "ready": ready,
            "safe_to_run": false,
            "activation_steps": activation_steps,
            "required_approvals": required_approvals,
            "risks": risks,
            "checks": checks,
            "guards": {
                "root": root,
                "registry_head": registry_head,
                "workflow_digest": workflow_digest,
                "skill_digests": skill_digests,
                "no_autonomous_execution": true,
            },
            "next_actions": observe_next_actions(
                "workflow.plan",
                [
                    format!("loom workflow preflight {}", shell_arg(&plan_id)),
                    "execution remains agent-driven until workflow apply gates are implemented".to_string()
                ],
            ),
        });
        let stored = StoredWorkflowPlan {
            plan_id: plan_id.clone(),
            schema_version: WORKFLOW_PLAN_SCHEMA.to_string(),
            protocol_version: PLAN_PROTOCOL_VERSION.to_string(),
            workflow_id: workflow.workflow_id.clone(),
            workflow_digest,
            registry_root: root,
            registry_head,
            agent,
            workspace: workspace.display().to_string(),
            skill_digests,
            created_at: Utc::now(),
            ready,
            payload: payload.clone(),
        };
        save_workflow_plan(&self.ctx, stored)?;
        Ok((payload, Meta::default()))
    }

    fn cmd_workflow_preflight(
        &self,
        args: &WorkflowPreflightArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_plan_id(&args.plan_id)?;
        let plans = load_workflow_plans(&self.ctx)?;
        let plan = plans.find(&args.plan_id).ok_or_else(|| {
            CommandFailure::new(
                ErrorCode::SkillNotFound,
                format!("workflow plan '{}' not found", args.plan_id),
            )
        })?;
        let workflows = load_workflows(&self.ctx)?;
        let workflow = find_workflow(&workflows, &plan.workflow_id)?;
        let mut checks = Vec::new();
        let current_root = canonical_root(&self.ctx.root)?;
        checks.push(check_value(
            "registry_root",
            current_root == plan.registry_root,
            json!({
                "expected": plan.registry_root,
                "actual": current_root,
            }),
        ));
        let current_head = gitops::head(&self.ctx).map_err(map_git)?;
        checks.push(check_value(
            "registry_head",
            current_head == plan.registry_head,
            json!({
                "expected": plan.registry_head,
                "actual": current_head,
            }),
        ));
        let current_workflow_digest = digest_workflow(workflow)?;
        checks.push(check_value(
            "workflow_digest",
            current_workflow_digest == plan.workflow_digest,
            json!({"expected": plan.workflow_digest, "actual": current_workflow_digest}),
        ));
        for (skill, expected_digest) in &plan.skill_digests {
            let skill_path = self.ctx.skill_path(skill);
            let (ok, actual) = if skill_path.is_dir() {
                let digest = skill_tree_digest(&skill_path).map_err(map_io)?;
                (digest == *expected_digest, Some(digest))
            } else {
                (false, None)
            };
            checks.push(check_value(
                &format!("skill_digest:{skill}"),
                ok,
                json!({"expected": expected_digest, "actual": actual}),
            ));
        }

        let valid = checks
            .iter()
            .all(|check| check["status"].as_str() == Some("pass"));
        Ok((
            json!({
                "plan_id": plan.plan_id,
                "workflow_id": plan.workflow_id,
                "valid": valid,
                "ready": plan.ready && valid,
                "safe_to_run": false,
                "checks": checks,
                "next_actions": observe_next_actions(
                    "workflow.preflight",
                    if valid {
                        vec!["manual execution remains deferred until workflow apply gates are implemented".to_string()]
                    } else {
                        vec![format!("rerun loom workflow plan {}", shell_arg(&plan.workflow_id))]
                    },
                ),
            }),
            Meta::default(),
        ))
    }

    fn cmd_workflow_run(
        &self,
        args: &WorkflowRunArgs,
    ) -> std::result::Result<(Value, Meta), CommandFailure> {
        validate_workflow_id(&args.name)?;
        let next_action = format!(
            "loom workflow plan {} --agent {} --workspace {}",
            shell_arg(&args.name),
            shell_arg(agent_kind_as_str(args.agent)),
            shell_arg(&args.workspace)
        );
        if args.dry_run {
            return Ok((
                json!({
                    "workflow_id": args.name,
                    "agent": agent_kind_as_str(args.agent),
                    "workspace": args.workspace,
                    "status": "deferred",
                    "deferred": true,
                    "hidden": true,
                    "safe_to_run": false,
                    "reason": "workflow execution is not public until workflow apply gates are implemented",
                    "next_actions": observe_next_actions(
                        "workflow.run.dry_deferred",
                        [next_action],
                    ),
                }),
                Meta::default(),
            ));
        }
        let mut failure = CommandFailure::new(
            ErrorCode::ArgInvalid,
            "workflow run is a hidden deferred compatibility surface",
        );
        failure.details = json!({
            "workflow_id": args.name,
            "agent": agent_kind_as_str(args.agent),
            "workspace": args.workspace,
            "status": "deferred",
            "deferred": true,
            "hidden": true,
            "safe_to_run": false,
            "reason": "workflow execution is not public until workflow apply gates are implemented",
            "next_actions": observe_next_actions(
                "workflow.run.error_deferred",
                [next_action],
            ),
        });
        Err(failure)
    }
}

fn workflow_from_file(
    requested_id: &str,
    path: &Path,
    now: DateTime<Utc>,
) -> std::result::Result<WorkflowRecord, CommandFailure> {
    let raw = fs::read_to_string(path).map_err(map_io)?;
    let input: WorkflowInput = serde_json::from_str(&raw).map_err(|err| {
        validation_error(
            "WORKFLOW_JSON_INVALID",
            format!(
                "failed to parse workflow JSON '{}': {}",
                path.display(),
                err
            ),
        )
    })?;
    workflow_from_input(requested_id, input, now)
}

fn workflow_from_skillset(
    ctx: &AppContext,
    requested_id: &str,
    skillset: Option<&str>,
    now: DateTime<Utc>,
) -> std::result::Result<WorkflowRecord, CommandFailure> {
    let skillset = skillset.ok_or_else(|| {
        validation_error(
            "WORKFLOW_SOURCE_INVALID",
            "provide a skillset id for --from-skillset",
        )
    })?;
    validate_workflow_id(skillset)?;
    let raw = fs::read_to_string(ctx.root.join("state/registry/skillsets.json")).map_err(map_io)?;
    let file: Value = serde_json::from_str(&raw).map_err(map_io)?;
    let Some(record) = file["skillsets"]
        .as_array()
        .and_then(|skillsets| skillsets.iter().find(|entry| entry["id"] == skillset))
    else {
        return Err(CommandFailure::new(
            ErrorCode::SkillNotFound,
            format!("skillset '{}' not found", skillset),
        ));
    };
    let members = record["members"].as_array().cloned().unwrap_or_default();
    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let mut previous = None;
    for member in members {
        let Some(skill_id) = member["skill_id"].as_str() else {
            continue;
        };
        let id = node_id_from_skill(skill_id);
        nodes.push(WorkflowNode {
            id: id.clone(),
            skill_id: skill_id.to_string(),
            kind: "skill".to_string(),
            requires: Vec::new(),
            outputs: vec![format!("{id}_result")],
            mutates_workspace: false,
        });
        if let Some(from) = previous.replace(id.clone()) {
            edges.push(WorkflowEdge { from, to: id });
        }
    }
    workflow_from_input(
        requested_id,
        WorkflowInput {
            workflow_id: Some(requested_id.to_string()),
            description: Some(format!("Workflow preview from skillset '{skillset}'")),
            nodes,
            edges,
            external_inputs: Vec::new(),
            policy: Default::default(),
        },
        now,
    )
}

fn workflow_from_input(
    requested_id: &str,
    input: WorkflowInput,
    now: DateTime<Utc>,
) -> std::result::Result<WorkflowRecord, CommandFailure> {
    let workflow_id = input
        .workflow_id
        .unwrap_or_else(|| requested_id.to_string());
    if workflow_id != requested_id {
        return Err(validation_error(
            "WORKFLOW_ID_MISMATCH",
            format!(
                "workflow id '{}' does not match requested id '{}'",
                workflow_id, requested_id
            ),
        ));
    }
    validate_workflow_id(&workflow_id)?;
    let mut record = WorkflowRecord {
        workflow_id,
        description: input.description.unwrap_or_default(),
        nodes: input.nodes,
        edges: input.edges,
        external_inputs: input.external_inputs,
        policy: input.policy,
        created_at: now,
        updated_at: now,
    };
    record.nodes.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(record)
}

fn skill_active_for_workspace(
    snapshot: &RegistrySnapshot,
    skill: &str,
    agent: &str,
    workspace: &Path,
) -> bool {
    snapshot.bindings.bindings.iter().any(|binding| {
        binding.agent == agent
            && binding.active
            && binding_matches_workspace(
                binding.workspace_matcher.kind.as_str(),
                &binding.workspace_matcher.value,
                workspace,
            )
            && snapshot.rules.rules.iter().any(|rule| {
                rule.binding_id == binding.binding_id
                    && rule.skill_id == skill
                    && snapshot.projections.projections.iter().any(|projection| {
                        projection.binding_id.as_deref() == Some(binding.binding_id.as_str())
                            && projection.skill_id == skill
                            && projection.health == crate::core::vocab::Health::Healthy
                    })
            })
    })
}

fn binding_matches_workspace(kind: &str, value: &str, workspace: &Path) -> bool {
    match kind {
        "path_prefix" => workspace.starts_with(Path::new(value)),
        "exact_path" => workspace == Path::new(value),
        "name" => value == "user",
        _ => false,
    }
}

fn add_policy_approvals(policy: &SkillPolicyReport, approvals: &mut BTreeSet<String>) {
    if policy.capabilities.filesystem.contains_key("write") {
        approvals.insert("filesystem-write".to_string());
    }
    if !policy.capabilities.shell.is_empty() {
        approvals.insert("shell".to_string());
    }
    if !policy.capabilities.network.is_empty() {
        approvals.insert("network".to_string());
    }
    if !policy.capabilities.secrets.is_empty() {
        approvals.insert("secrets".to_string());
    }
    if policy.summary.high_risk_count > 0 {
        approvals.insert("policy-high-risk".to_string());
    }
}

fn add_policy_risks(node_id: &str, policy: &SkillPolicyReport, risks: &mut Vec<Value>) {
    for finding in &policy.findings {
        risks.push(json!({
            "node_id": node_id,
            "skill": policy.skill,
            "code": finding.id,
            "risk_level": finding.risk_level,
            "blocks_apply": finding.blocks_projection,
            "details": finding.details,
        }));
    }
}

fn render_workflow(workflow: &WorkflowRecord, order: &[String]) -> Value {
    json!({
        "workflow_id": workflow.workflow_id,
        "description": workflow.description,
        "nodes": workflow.nodes,
        "edges": workflow.edges,
        "external_inputs": workflow.external_inputs,
        "policy": workflow.policy,
        "ordered_node_ids": order,
        "created_at": workflow.created_at.to_rfc3339(),
        "updated_at": workflow.updated_at.to_rfc3339(),
    })
}

fn digest_workflow(workflow: &WorkflowRecord) -> std::result::Result<String, CommandFailure> {
    let mut normalized = workflow.clone();
    normalized.created_at = DateTime::<Utc>::UNIX_EPOCH;
    normalized.updated_at = DateTime::<Utc>::UNIX_EPOCH;
    let raw = serde_json::to_vec(&normalized).map_err(map_io)?;
    let mut hasher = Sha256::new();
    hasher.update(&raw);
    Ok(format!("sha256:{}", to_hex(&hasher.finalize())))
}

fn check_value(id: &str, ok: bool, details: Value) -> Value {
    json!({
        "id": id,
        "status": if ok { "pass" } else { "fail" },
        "details": details,
    })
}

fn absolute_path(path: &Path) -> std::result::Result<PathBuf, CommandFailure> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir().map_err(map_io)?.join(path))
    }
}

fn canonical_root(root: &Path) -> std::result::Result<String, CommandFailure> {
    Ok(fs::canonicalize(root)
        .map_err(map_io)?
        .display()
        .to_string())
}

fn approval_token(node_id: &str) -> String {
    format!("approve-{}", slugify(node_id))
}

fn node_id_from_skill(skill: &str) -> String {
    slugify(skill).replace('_', "-")
}
