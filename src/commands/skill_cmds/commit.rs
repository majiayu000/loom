use tar::Archive;

use crate::error_actions::NextAction;

use super::shared::*;
use super::*;

impl App {
    pub fn cmd_commit(
        &self,
        args: &SkillCommitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        validate_skill_name(&args.skill).map_err(map_arg)?;
        self.ensure_write_repo_ready()?;
        if args.from_source {
            return self.commit_from_source(args, request_id);
        }
        if args.from_projection {
            return self.commit_from_projection(args, request_id);
        }

        let source_dirty = !source_dirty_paths(&self.ctx, &args.skill)?.is_empty();
        let dirty_projections = dirty_projection_candidates(&self.ctx, args)?;
        if source_dirty && !dirty_projections.is_empty() {
            let mut failure = CommandFailure::new(
                ErrorCode::CommitDirectionAmbiguous,
                format!(
                    "skill '{}' has both source and projection changes; choose a commit direction",
                    args.skill
                ),
            );
            failure.details = json!({
                "skill": args.skill,
                "source_dirty": true,
                "projection_dirty": true,
                "dirty_projections": dirty_projections.iter().map(projection_summary).collect::<Vec<_>>(),
            });
            let mut next_actions = vec![NextAction {
                cmd: format!(
                    "loom skill commit {} --from-source --json",
                    shell_arg(&args.skill)
                ),
                reason: "commit registry source changes".to_string(),
            }];
            next_actions.extend(dirty_projections.iter().map(|projection| NextAction {
                cmd: projection_commit_command(&args.skill, &projection.instance_id),
                reason: format!(
                    "capture live projection changes from instance {}",
                    projection.instance_id
                ),
            }));
            failure.next_actions = next_actions;
            return Err(failure);
        }
        if source_dirty {
            return self.commit_from_source(args, request_id);
        }
        if let Some(projection) = select_projection(args, &dirty_projections)? {
            return self.capture_selected_projection(args, request_id, projection);
        }
        Ok((
            json!({
                "skill": args.skill,
                "direction": Value::Null,
                "source_dirty": false,
                "projection_dirty": false,
                "noop": true,
            }),
            Meta::default(),
        ))
    }

    fn commit_from_source(
        &self,
        args: &SkillCommitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let save_args = SaveArgs {
            skill: args.skill.clone(),
            message: args.message.clone(),
            preflight: args.preflight,
        };
        let (mut payload, meta) = self.cmd_save(&save_args, request_id)?;
        payload["direction"] = json!("source");
        Ok((payload, meta))
    }

    fn commit_from_projection(
        &self,
        args: &SkillCommitArgs,
        request_id: &str,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let projections = matching_projection_candidates(&self.ctx, args)?;
        let dirty = dirty_projection_candidates(&self.ctx, args)?;
        let selected = if let Some(projection) = select_projection(args, &dirty)? {
            Some(projection)
        } else if projections.len() == 1 {
            projections.first().cloned()
        } else {
            None
        };
        let Some(projection) = selected else {
            return Err(projection_selection_error(args, projections));
        };
        self.capture_selected_projection(args, request_id, projection)
    }

    fn capture_selected_projection(
        &self,
        args: &SkillCommitArgs,
        request_id: &str,
        projection: RegistryProjectionInstance,
    ) -> std::result::Result<(serde_json::Value, Meta), CommandFailure> {
        let capture_args = CaptureArgs {
            skill: Some(args.skill.clone()),
            binding: projection.binding_id.clone(),
            instance: Some(projection.instance_id.clone()),
            message: args.message.clone(),
            dry_run: false,
        };
        let (mut payload, meta) = self.cmd_capture(&capture_args, request_id)?;
        payload["direction"] = json!("projection");
        Ok((payload, meta))
    }
}

fn source_dirty_paths(
    ctx: &crate::state::AppContext,
    skill: &str,
) -> std::result::Result<Vec<String>, CommandFailure> {
    let prefix = format!("skills/{skill}");
    let mut paths = Vec::new();
    if git_head_exists(ctx)? {
        collect_git_paths(
            ctx,
            &["diff", "--name-only", "HEAD", "--", &prefix],
            &mut paths,
        )?;
        collect_git_paths(
            ctx,
            &["diff", "--name-only", "--cached", "--", &prefix],
            &mut paths,
        )?;
    }
    collect_git_paths(
        ctx,
        &["ls-files", "--others", "--exclude-standard", "--", &prefix],
        &mut paths,
    )?;
    paths.sort();
    Ok(paths)
}

fn collect_git_paths(
    ctx: &crate::state::AppContext,
    args: &[&str],
    paths: &mut Vec<String>,
) -> std::result::Result<(), CommandFailure> {
    let stdout = gitops::run_git(ctx, args).map_err(map_git)?;
    for line in stdout.lines() {
        let path = line.trim();
        if !path.is_empty() && !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    Ok(())
}

fn git_head_exists(ctx: &crate::state::AppContext) -> std::result::Result<bool, CommandFailure> {
    let output =
        gitops::run_git_allow_failure(ctx, &["rev-parse", "--verify", "HEAD"]).map_err(map_git)?;
    Ok(output.status.success())
}

fn matching_projection_candidates(
    ctx: &crate::state::AppContext,
    args: &SkillCommitArgs,
) -> std::result::Result<Vec<RegistryProjectionInstance>, CommandFailure> {
    let paths = RegistryStatePaths::from_app_context(ctx);
    let Some(snapshot) = paths.maybe_load_snapshot().map_err(map_registry_state)? else {
        return Ok(Vec::new());
    };
    Ok(snapshot
        .projections
        .projections
        .into_iter()
        .filter(|projection| projection.skill_id == args.skill)
        .filter(|projection| {
            args.binding
                .as_deref()
                .is_none_or(|binding| projection.binding_id.as_deref() == Some(binding))
        })
        .filter(|projection| {
            args.instance
                .as_deref()
                .is_none_or(|instance| projection.instance_id == instance)
        })
        .collect())
}

fn dirty_projection_candidates(
    ctx: &crate::state::AppContext,
    args: &SkillCommitArgs,
) -> std::result::Result<Vec<RegistryProjectionInstance>, CommandFailure> {
    let mut dirty = Vec::new();
    for projection in matching_projection_candidates(ctx, args)? {
        if projection.method == crate::core::vocab::ProjectionMethod::Symlink {
            continue;
        }
        let live_path = PathBuf::from(&projection.materialized_path);
        if !live_path.is_dir() {
            continue;
        }
        if projection_differs_from_applied(ctx, &projection, &live_path)? {
            dirty.push(projection);
        }
    }
    Ok(dirty)
}

fn projection_differs_from_applied(
    ctx: &crate::state::AppContext,
    projection: &RegistryProjectionInstance,
    live_path: &Path,
) -> std::result::Result<bool, CommandFailure> {
    let temp_root = std::env::temp_dir().join(format!("loom-commit-projection-{}", Uuid::new_v4()));
    fs::create_dir_all(&temp_root).map_err(map_io)?;
    let result = (|| {
        materialize_skill_at_ref(
            ctx,
            &projection.skill_id,
            &projection.last_applied_rev,
            &temp_root,
        )?;
        materialized_dirs_equal(
            &temp_root.join("skills").join(&projection.skill_id),
            live_path,
        )
        .map(|equal| !equal)
        .map_err(map_io)
    })();
    let _ = fs::remove_dir_all(&temp_root);
    result
}

fn materialize_skill_at_ref(
    ctx: &crate::state::AppContext,
    skill: &str,
    reference: &str,
    root: &Path,
) -> std::result::Result<(), CommandFailure> {
    let skill_rel = format!("skills/{skill}");
    let output = gitops::run_git_allow_failure(
        ctx,
        &["archive", "--format=tar", reference, "--", &skill_rel],
    )
    .map_err(map_git)?;
    if !output.status.success() {
        return Err(map_git(anyhow::anyhow!(
            "git archive failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Archive::new(&output.stdout[..])
        .unpack(root)
        .map_err(map_io)
}

fn select_projection(
    args: &SkillCommitArgs,
    candidates: &[RegistryProjectionInstance],
) -> std::result::Result<Option<RegistryProjectionInstance>, CommandFailure> {
    match candidates.len() {
        0 => Ok(None),
        1 => Ok(candidates.first().cloned()),
        _ => Err(projection_selection_error(args, candidates.to_vec())),
    }
}

fn projection_selection_error(
    args: &SkillCommitArgs,
    candidates: Vec<RegistryProjectionInstance>,
) -> CommandFailure {
    let mut failure = CommandFailure::new(
        ErrorCode::ArgInvalid,
        format!(
            "multiple projections match skill '{}'; pass --binding or --instance",
            args.skill
        ),
    );
    failure.details = json!({
        "skill": args.skill,
        "projections": candidates.iter().map(projection_summary).collect::<Vec<_>>(),
    });
    failure
}

fn projection_summary(projection: &RegistryProjectionInstance) -> Value {
    json!({
        "instance_id": projection.instance_id,
        "binding_id": projection.binding_id,
        "method": projection.method,
        "materialized_path": projection.materialized_path,
    })
}

fn projection_commit_command(skill: &str, instance_id: &str) -> String {
    format!(
        "loom skill commit {} --from-projection --instance {} --json",
        shell_arg(skill),
        shell_arg(instance_id)
    )
}

#[cfg(test)]
mod tests {
    use super::projection_commit_command;

    #[test]
    fn projection_commit_action_quotes_untrusted_instance_id() {
        assert_eq!(
            projection_commit_command("model-onboarding", "inst; touch /tmp/owned"),
            "loom skill commit model-onboarding --from-projection --instance 'inst; touch /tmp/owned' --json"
        );
    }
}
