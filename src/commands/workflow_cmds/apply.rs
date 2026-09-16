use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{WorkflowApplyArgs, WorkflowPreflightArgs};
use crate::commands::helpers::{map_io, map_lock, validate_non_empty};
use crate::commands::skill_authoring::sha256_digest;
use crate::commands::skill_deps::skill_dependency_report;
use crate::commands::skill_eval_harness::execute_reviewed_prompt;
use crate::commands::skill_safety::enforce_skill_safety;
use crate::commands::{App, CommandFailure, redact_sensitive_string};
use crate::envelope::Meta;
use crate::state_model::RegistryStatePaths;
use crate::types::ErrorCode;

use super::model::WorkflowExecution;
use super::store::{find_workflow, load_workflow_plans, load_workflows, save_workflow_plan};
use super::validate::{validate_plan_id, validate_workflow_definition, workflow_node};
use super::{canonical_root, skill_active_for_workspace};

impl App {
    pub(super) fn cmd_workflow_apply(
        &self,
        args: &WorkflowApplyArgs,
    ) -> Result<(Value, Meta), CommandFailure> {
        validate_plan_id(&args.plan_id)?;
        validate_non_empty("idempotency-key", &args.idempotency_key)?;
        self.ctx.ensure_not_loom_tool_repo_root().map_err(map_io)?;
        let _lock = self.ctx.lock_workspace().map_err(map_lock)?;
        let plans = load_workflow_plans(&self.ctx)?;
        let mut plan = plans
            .find(&args.plan_id)
            .cloned()
            .ok_or_else(|| invalid("workflow plan not found"))?;
        if plan.agent != "codex" {
            return Err(invalid(
                "workflow apply currently requires a plan for agent codex",
            ));
        }
        let inputs: BTreeMap<String, String> = match &args.inputs {
            Some(path) => serde_json::from_str(&fs::read_to_string(path).map_err(map_io)?)
                .map_err(|_| invalid("--inputs must be a JSON object of named string values"))?,
            None => BTreeMap::new(),
        };
        let inputs_digest = sha256_digest(&serde_json::to_vec(&inputs).map_err(map_io)?);
        let key_digest = sha256_digest(args.idempotency_key.as_bytes());
        if plans.plans.iter().any(|other| {
            other.plan_id != plan.plan_id
                && other
                    .execution
                    .as_ref()
                    .is_some_and(|record| record.key_digest == key_digest)
        }) {
            return Err(invalid(
                "idempotency key already belongs to another workflow plan",
            ));
        }
        if let Some(execution) = &plan.execution {
            if execution.key_digest != key_digest || execution.inputs_digest != inputs_digest {
                return Err(invalid(
                    "workflow plan was already applied with different inputs or idempotency key",
                ));
            }
            let result = json!({"plan_id": plan.plan_id, "replayed": true, "execution": execution});
            return if execution.status == "completed" {
                Ok((result, Meta::default()))
            } else {
                Err(stopped(
                    "workflow execution is incomplete; inspect recorded nodes and checkpoints before creating a new plan",
                    result,
                ))
            };
        }

        let (preflight, _) = self.cmd_workflow_preflight(&WorkflowPreflightArgs {
            plan_id: plan.plan_id.clone(),
        })?;
        if preflight["valid"] != true {
            return Err(stopped(
                "workflow plan is stale; create and review a new plan",
                preflight,
            ));
        }
        let workflows = load_workflows(&self.ctx)?;
        let workflow = find_workflow(&workflows, &plan.workflow_id)?;
        let order = validate_workflow_definition(workflow)?;
        for name in &workflow.external_inputs {
            if inputs.get(name).is_none_or(|value| value.trim().is_empty()) {
                return Err(invalid(&format!("missing workflow input '{name}'")));
            }
        }
        let workspace = fs::canonicalize(&plan.workspace).map_err(map_io)?;
        let registry = fs::canonicalize(&self.ctx.root).map_err(map_io)?;
        if workspace.starts_with(&registry) || registry.starts_with(&workspace) {
            return Err(invalid(
                "workflow workspace and registry must be separate directories",
            ));
        }
        let git_root = git(&workspace, &["rev-parse", "--show-toplevel"], None)?;
        if canonical_root(Path::new(git_root.trim()))? != canonical_root(&workspace)? {
            return Err(invalid("workflow workspace must be the Git worktree root"));
        }
        let required = plan.payload["required_approvals"]
            .as_array()
            .ok_or_else(|| invalid("workflow plan has no approval list"))?;
        let missing = required
            .iter()
            .filter_map(Value::as_str)
            .filter(|approval| !args.approve.iter().any(|given| given == approval))
            .collect::<Vec<_>>();
        let snapshot = RegistryStatePaths::from_app_context(&self.ctx)
            .load_snapshot()
            .map_err(map_io)?;
        for id in &order {
            let node = workflow_node(workflow, id)?;
            enforce_skill_safety(&self.ctx, &node.skill_id, "safe-capture")?;
            let dependencies = skill_dependency_report(
                &self.ctx,
                &node.skill_id,
                Some("codex"),
                Some(&workspace),
            )?;
            if !dependencies.ready {
                return Err(stopped(
                    "workflow skill dependencies are not ready",
                    json!(dependencies),
                ));
            }
            if !skill_active_for_workspace(&snapshot, &node.skill_id, "codex", &workspace)? {
                return Err(invalid(&format!(
                    "skill '{}' is not active for this workspace; activate it and create a fresh plan",
                    node.skill_id
                )));
            }
        }
        if args.dry_run {
            return Ok((
                json!({"plan_id": plan.plan_id, "dry_run": true, "runner": "codex-cli",
                "ready": missing.is_empty(), "missing_approvals": missing, "ordered_node_ids": order}),
                Meta::default(),
            ));
        }
        if !missing.is_empty() {
            return Err(stopped(
                "workflow requires the reviewed approvals",
                json!({"missing_approvals": missing}),
            ));
        }
        plan.execution = Some(WorkflowExecution {
            key_digest,
            inputs_digest,
            status: "running".to_string(),
            nodes: Vec::new(),
            error: None,
        });
        save_workflow_plan(&self.ctx, plan.clone())?;
        let mut values = inputs;
        for id in order {
            let node = workflow_node(workflow, &id)?;
            let result: Result<(), CommandFailure> = (|| {
                // Recheck the frozen source between calls: another process can edit source files.
                let (current, _) = self.cmd_workflow_preflight(&WorkflowPreflightArgs {
                    plan_id: plan.plan_id.clone(),
                })?;
                if current["valid"] != true {
                    return Err(stopped("workflow source changed during execution", current));
                }
                let checkpoint = if node.mutates_workspace {
                    Some(checkpoint(&workspace, &id)?)
                } else {
                    None
                };
                let source =
                    fs::read_to_string(self.ctx.skill_path(&node.skill_id).join("SKILL.md"))
                        .map_err(map_io)?;
                let needed = node
                    .requires
                    .iter()
                    .map(|name| {
                        values
                            .get(name)
                            .map(|value| (name.clone(), value.clone()))
                            .ok_or_else(|| {
                                invalid(&format!("node '{id}' requires missing output '{name}'"))
                            })
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                let execution = plan
                    .execution
                    .as_mut()
                    .ok_or_else(|| invalid("execution record missing"))?;
                execution
                    .nodes
                    .push(json!({"id": id, "status": "running", "checkpoint_ref": checkpoint}));
                save_workflow_plan(&self.ctx, plan.clone())?;
                let prompt = format!(
                    "Execute this reviewed Loom workflow node: {id}. Work only inside the current project. \
                     Do not run Loom, change Git history, or modify the registry. \
                     Workspace writes permitted: {}. Follow the supplied skill and named inputs. \
                     Return only a JSON object mapping each requested output name to a string result. \
                     Requested output names: {}\nInputs:\n{}\nSkill:\n{}",
                    node.mutates_workspace,
                    json!(node.outputs),
                    json!(needed),
                    source,
                );
                let output =
                    execute_reviewed_prompt(Some(&workspace), &prompt, node.mutates_workspace)?;
                let outputs: BTreeMap<String, String> = serde_json::from_str(&output)
                    .map_err(|_| invalid(&format!("node '{id}' returned invalid output JSON")))?;
                for name in &node.outputs {
                    let value = outputs
                        .get(name)
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| {
                            invalid(&format!("node '{id}' did not produce output '{name}'"))
                        })?;
                    values.insert(name.clone(), value.clone());
                }
                let execution = plan
                    .execution
                    .as_mut()
                    .ok_or_else(|| invalid("execution record missing"))?;
                let record = execution
                    .nodes
                    .last_mut()
                    .ok_or_else(|| invalid("node record missing"))?;
                record["status"] = json!("completed");
                record["outputs"] = json!(
                    outputs
                        .iter()
                        .map(|(key, value)| (key, redact_sensitive_string(value)))
                        .collect::<BTreeMap<_, _>>()
                );
                save_workflow_plan(&self.ctx, plan.clone())?;
                Ok(())
            })();
            if let Err(failure) = result {
                let execution = plan
                    .execution
                    .as_mut()
                    .ok_or_else(|| invalid("execution record missing"))?;
                execution.status = "failed".to_string();
                execution.error = Some(redact_sensitive_string(&failure.message));
                if let Some(record) = execution
                    .nodes
                    .last_mut()
                    .filter(|record| record["id"] == id)
                {
                    record["status"] = json!("failed");
                }
                save_workflow_plan(&self.ctx, plan.clone())?;
                return Err(stopped(
                    "workflow stopped; workspace changes are retained for inspection and recovery",
                    json!({"plan_id": plan.plan_id, "failed_node": id, "execution": plan.execution, "cause": failure.message}),
                ));
            }
        }
        plan.execution
            .as_mut()
            .ok_or_else(|| invalid("execution record missing"))?
            .status = "completed".to_string();
        save_workflow_plan(&self.ctx, plan.clone())?;
        Ok((
            json!({"plan_id": plan.plan_id, "replayed": false, "execution": plan.execution}),
            Meta::default(),
        ))
    }
}

/// Capture tracked and non-ignored files using a separate index, preserving the user's index.
fn checkpoint(workspace: &Path, node: &str) -> Result<String, CommandFailure> {
    let index = std::env::temp_dir().join(format!("loom-workflow-index-{}", Uuid::new_v4()));
    let result = (|| {
        git(workspace, &["read-tree", "HEAD"], Some(&index))?;
        git(workspace, &["add", "-A", "--", "."], Some(&index))?;
        let tree = git(workspace, &["write-tree"], Some(&index))?;
        let parent = git(workspace, &["rev-parse", "HEAD"], None)?;
        let commit = git(
            workspace,
            &[
                "commit-tree",
                tree.trim(),
                "-p",
                parent.trim(),
                "-m",
                &format!("Workflow checkpoint before {node}"),
            ],
            None,
        )?;
        let reference = format!("refs/loom/workflow-checkpoints/{}", Uuid::new_v4().simple());
        git(workspace, &["update-ref", &reference, commit.trim()], None)?;
        Ok(reference)
    })();
    match fs::remove_file(&index) {
        Ok(()) => result,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => result,
        Err(err) => Err(CommandFailure::new(
            ErrorCode::IoError,
            format!("workflow checkpoint index cleanup failed: {err}"),
        )),
    }
}

fn git(workspace: &Path, args: &[&str], index: Option<&Path>) -> Result<String, CommandFailure> {
    let mut command = Command::new("git");
    command.current_dir(workspace).args(args);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let output = command.output().map_err(map_io)?;
    if !output.status.success() {
        return Err(CommandFailure::new(
            ErrorCode::GitError,
            format!(
                "workflow Git operation failed: {}",
                redact_sensitive_string(&String::from_utf8_lossy(&output.stderr))
            ),
        ));
    }
    String::from_utf8(output.stdout).map_err(map_io)
}

fn invalid(message: &str) -> CommandFailure {
    CommandFailure::new(ErrorCode::ArgInvalid, message)
}

fn stopped(message: &str, details: Value) -> CommandFailure {
    let mut failure = CommandFailure::new(ErrorCode::PolicyBlocked, message);
    failure.details = details;
    failure
}
