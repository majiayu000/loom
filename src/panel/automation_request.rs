//! Fixed operation requests shared by the HTTP panel and desktop sidecar bridge.
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum AutomationRequest {
    WorkflowCreate {
        name: String,
        skillset: String,
        dry_run: bool,
    },
    WorkflowPlan {
        name: String,
        workspace: String,
    },
    WorkflowApply {
        plan: String,
        inputs: Option<String>,
        approvals: Vec<String>,
        idempotency_key: String,
        dry_run: bool,
    },
    SkillsetEval {
        name: String,
        runner: String,
        baseline: String,
        dry_run: bool,
    },
    AuthorDraft {
        name: String,
        session: String,
        dry_run: bool,
    },
    AuthorRewrite {
        name: String,
        instruction: String,
        dry_run: bool,
    },
    AuthorApply {
        patch: String,
        idempotency_key: String,
    },
    InstructionScan {
        workspace: String,
    },
    InstructionMigrate {
        instruction_id: String,
        workspace: String,
        name: String,
        target: String,
        dry_run: bool,
    },
    PackagePlan {
        source: String,
        format: String,
        output: String,
    },
    PackageBuild {
        plan: String,
        output: String,
        idempotency_key: String,
    },
    PackageVerify {
        artifact: String,
    },
    ProvisionPlan {
        target: String,
        workspace: String,
    },
    ProvisionExport {
        plan: String,
        format: String,
        output: String,
    },
    ProvisionImport {
        artifact: String,
        output: String,
        dry_run: bool,
    },
    ProvisionApply {
        plan: String,
        approvals: Vec<String>,
        idempotency_key: String,
    },
    CatalogSearch {
        provider: String,
        query: String,
    },
    CatalogPreview {
        locator: String,
    },
}

type CommandParts = (
    &'static [&'static str],
    Option<String>,
    Vec<(&'static str, String)>,
    bool,
);

impl AutomationRequest {
    pub(crate) fn argv(self) -> Vec<String> {
        let (prefix, positional, flags, dry_run): CommandParts = match self {
            Self::WorkflowCreate {
                name,
                skillset,
                dry_run,
            } => (
                &["workflow", "create"],
                Some(name),
                vec![("from-skillset", skillset)],
                dry_run,
            ),
            Self::WorkflowPlan { name, workspace } => (
                &["workflow", "plan"],
                Some(name),
                vec![("workspace", workspace), ("agent", "codex".into())],
                false,
            ),
            Self::WorkflowApply {
                plan,
                inputs,
                approvals,
                idempotency_key,
                dry_run,
            } => {
                let mut flags = vec![("idempotency-key", idempotency_key)];
                if let Some(inputs) = inputs.filter(|s| !s.is_empty()) {
                    flags.push(("inputs", inputs));
                }
                flags.extend(approvals.into_iter().map(|a| ("approve", a)));
                (&["workflow", "apply"], Some(plan), flags, dry_run)
            }
            Self::SkillsetEval {
                name,
                runner,
                baseline,
                dry_run,
            } => (
                &["skillset", "eval"],
                Some(name),
                vec![
                    ("runner", runner),
                    ("baseline", baseline),
                    ("agent", "codex".into()),
                ],
                dry_run,
            ),
            Self::AuthorDraft {
                name,
                session,
                dry_run,
            } => (
                &["skill", "author", "draft"],
                Some(name),
                vec![("from-session", session), ("provider", "codex-cli".into())],
                dry_run,
            ),
            Self::AuthorRewrite {
                name,
                instruction,
                dry_run,
            } => (
                &["skill", "author", "rewrite"],
                Some(name),
                vec![
                    ("instruction", instruction),
                    ("provider", "codex-cli".into()),
                ],
                dry_run,
            ),
            Self::AuthorApply {
                patch,
                idempotency_key,
            } => (
                &["skill", "author", "apply-patch"],
                Some(patch),
                vec![("idempotency-key", idempotency_key)],
                false,
            ),
            Self::InstructionScan { workspace } => (
                &["instruction", "scan"],
                None,
                vec![("workspace", workspace)],
                false,
            ),
            Self::InstructionMigrate {
                instruction_id,
                workspace,
                name,
                target,
                dry_run,
            } => (
                &["instruction", "migrate-plan"],
                Some(instruction_id),
                vec![("workspace", workspace), ("name", name), ("to", target)],
                dry_run,
            ),
            Self::PackagePlan {
                source,
                format,
                output,
            } => (
                &["package", "plan"],
                Some(source),
                vec![("format", format), ("output-plan", output)],
                false,
            ),
            Self::PackageBuild {
                plan,
                output,
                idempotency_key,
            } => (
                &["package", "build"],
                Some(plan),
                vec![("output", output), ("idempotency-key", idempotency_key)],
                false,
            ),
            Self::PackageVerify { artifact } => {
                (&["package", "verify"], Some(artifact), vec![], false)
            }
            Self::ProvisionPlan { target, workspace } => (
                &["provision", "plan"],
                None,
                vec![("target", target), ("workspace", workspace)],
                false,
            ),
            Self::ProvisionExport {
                plan,
                format,
                output,
            } => (
                &["provision", "export"],
                Some(plan),
                vec![("format", format), ("output", output)],
                false,
            ),
            Self::ProvisionImport {
                artifact,
                output,
                dry_run,
            } => (
                &["provision", "import"],
                Some(artifact),
                vec![("output", output)],
                dry_run,
            ),
            Self::ProvisionApply {
                plan,
                approvals,
                idempotency_key,
            } => {
                let mut flags = vec![("idempotency-key", idempotency_key)];
                flags.extend(approvals.into_iter().map(|a| ("approve", a)));
                (&["provision", "apply"], Some(plan), flags, false)
            }
            Self::CatalogSearch { provider, query } => (
                &["catalog", "search", "--allow-network"],
                Some(query),
                vec![("provider", provider)],
                false,
            ),
            Self::CatalogPreview { locator } => {
                (&["catalog", "preview"], Some(locator), vec![], false)
            }
        };
        let mut argv = prefix.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Joined flag values and a positional terminator keep user text out of option parsing.
        argv.extend(
            flags
                .into_iter()
                .map(|(key, value)| format!("--{key}={value}")),
        );
        if dry_run {
            argv.push("--dry-run".into());
        }
        if let Some(value) = positional {
            argv.extend(["--".into(), value]);
        }
        argv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_arbitrary_commands_and_unknown_fields() {
        let shell = json!({"action":"shell","command":"echo bad"});
        let root = json!({"action":"package_verify","artifact":"a","root":"/elsewhere"});
        assert!(serde_json::from_value::<AutomationRequest>(shell).is_err());
        assert!(serde_json::from_value::<AutomationRequest>(root).is_err());
    }

    #[test]
    fn user_text_cannot_change_command_or_global_options() {
        let request:AutomationRequest=serde_json::from_value(json!({"action":"author_rewrite","name":"--root=/elsewhere","instruction":"--provider=mock","dry_run":true})).unwrap();
        assert_eq!(
            request.argv(),
            vec![
                "skill",
                "author",
                "rewrite",
                "--instruction=--provider=mock",
                "--provider=codex-cli",
                "--dry-run",
                "--",
                "--root=/elsewhere"
            ]
        );
    }
}
