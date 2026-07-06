use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::commands::{CommandFailure, redact_sensitive_string};
use crate::sha256::{Sha256, to_hex};
use crate::types::ErrorCode;

use super::cases::{HarnessJsonlRecord, HarnessTaskCase, HarnessTriggerCase, HarnessTriggerResult};
use super::report::{eval_failed, harness_schema_failure, io_failure};
use super::runner::{
    CleanupResult, EvalCaseResult, EvalPlan, EvalRunEnvironment, EvalVariant, Metrics,
    SkillEvalRunner, case_status_score, grade_eval_result, isolated_workspace, prepare_workspace,
    slug_path_component,
};

const DEFAULT_CODEX_TIMEOUT_MS: u64 = 120_000;

pub(super) struct CodexCliRunner;

impl SkillEvalRunner for CodexCliRunner {
    fn prepare(
        &mut self,
        _plan: &EvalPlan,
    ) -> std::result::Result<EvalRunEnvironment, CommandFailure> {
        let path = std::env::temp_dir().join(format!("loom-eval-codex-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).map_err(|err| io_failure("eval_temp_prepare", &path, err))?;
        Ok(EvalRunEnvironment { root: path })
    }

    fn run_case(
        &mut self,
        env: &EvalRunEnvironment,
        plan: &EvalPlan,
        case: &HarnessTaskCase,
        case_key: &str,
        variant: EvalVariant,
        attempt: u32,
    ) -> std::result::Result<EvalCaseResult, CommandFailure> {
        let workspace = isolated_workspace(env, case_key, variant, attempt);
        prepare_workspace(plan, case, &workspace)?;
        let before = WorkspaceSnapshot::capture(&workspace)?;
        let prompt = task_prompt(plan, case, variant);
        let executed = execute_codex_jsonl(env, &workspace, &prompt)?;
        let files_changed = before.changed_files(&workspace)?;
        let output = executed.final_output();
        let metrics = Metrics {
            tokens: executed.trace.tokens,
            commands: Some(executed.trace.commands.len() as u64),
            duration_ms: Some(executed.duration_ms),
        };
        let checks = grade_eval_result(
            case,
            &output,
            executed.exit_code,
            &executed.trace.commands,
            &files_changed,
            metrics,
        );
        let (status, score) = case_status_score(&checks);
        Ok(EvalCaseResult {
            id: case.id(),
            attempt,
            variant,
            status,
            score,
            output: redact_sensitive_string(&output),
            exit_code: executed.exit_code,
            commands: executed.trace.commands,
            files_changed,
            metrics,
            checks,
            workspace: workspace.display().to_string(),
        })
    }

    fn run_trigger_case(
        &mut self,
        env: &EvalRunEnvironment,
        plan: &EvalPlan,
        attempt: u32,
        record: &HarnessJsonlRecord<HarnessTriggerCase>,
    ) -> std::result::Result<HarnessTriggerResult, CommandFailure> {
        let expected = record.value.expected_trigger().ok_or_else(|| {
            harness_schema_failure(
                "trigger eval case requires expected_trigger, should_trigger, expected, or label",
                Path::new("evals/triggers.jsonl"),
                record.line,
            )
        })?;
        let prompt = record.value.prompt.as_ref().ok_or_else(|| {
            harness_schema_failure(
                "trigger eval case requires an input, prompt, or text field",
                Path::new("evals/triggers.jsonl"),
                record.line,
            )
        })?;
        let workspace = env.root.join(format!(
            "trigger-line{}-{}-{}",
            record.line,
            slug_path_component(record.value.id.as_deref().unwrap_or("trigger")),
            attempt
        ));
        fs::create_dir_all(&workspace)
            .map_err(|err| io_failure("eval_trigger_workspace_create", &workspace, err))?;
        let executed = execute_codex_jsonl(env, &workspace, &trigger_prompt(plan, prompt))?;
        let observed = parse_trigger_decision(&executed.final_output())?;
        Ok(HarnessTriggerResult {
            id: record
                .value
                .id
                .clone()
                .unwrap_or_else(|| format!("trigger-{}", record.line)),
            line: record.line,
            agent: plan.agent.clone(),
            attempt,
            prompt: "<redacted>".to_string(),
            expected_trigger: expected,
            observed_trigger: observed,
            status: if expected == observed {
                "passed"
            } else {
                "failed"
            },
        })
    }

    fn cleanup(&mut self, env: EvalRunEnvironment) -> CleanupResult {
        match fs::remove_dir_all(&env.root) {
            Ok(()) => CleanupResult::passed("temporary codex-cli eval workspaces removed"),
            Err(err) => CleanupResult::failure(&format!(
                "failed to remove temporary codex-cli eval workspaces: {err}"
            )),
        }
    }
}

struct CodexExecution {
    trace: CodexTrace,
    last_message: Option<String>,
    stderr: String,
    exit_code: i32,
    duration_ms: u64,
}

impl CodexExecution {
    fn final_output(&self) -> String {
        let mut output = self
            .last_message
            .as_ref()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| self.trace.output.join("\n"));
        if output.trim().is_empty() && !self.stderr.trim().is_empty() {
            output = self.stderr.clone();
        }
        output
    }
}

#[derive(Default)]
struct CodexTrace {
    output: Vec<String>,
    commands: Vec<String>,
    tokens: Option<u64>,
}

fn execute_codex_jsonl(
    env: &EvalRunEnvironment,
    workspace: &Path,
    prompt: &str,
) -> std::result::Result<CodexExecution, CommandFailure> {
    let timeout = codex_timeout()?;
    let trace_id = Uuid::new_v4();
    let stdout_path = env.root.join(format!("{trace_id}.stdout.jsonl"));
    let stderr_path = env.root.join(format!("{trace_id}.stderr.txt"));
    let last_message_path = env.root.join(format!("{trace_id}.last-message.txt"));
    let stdout = File::create(&stdout_path)
        .map_err(|err| io_failure("codex_trace_stdout_create", &stdout_path, err))?;
    let stderr = File::create(&stderr_path)
        .map_err(|err| io_failure("codex_trace_stderr_create", &stderr_path, err))?;
    let started = Instant::now();
    let mut child = Command::new("codex")
        .arg("exec")
        .arg("--json")
        .arg("--cd")
        .arg(workspace)
        .arg("--skip-git-repo-check")
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--output-last-message")
        .arg(&last_message_path)
        .arg(prompt)
        .current_dir(workspace)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|err| {
            eval_failed(
                "codex-cli runner failed to spawn",
                0,
                "runner_spawn_failed",
                json!({"runner": "codex-cli", "error": err.to_string()}),
            )
        })?;

    let status = loop {
        if let Some(status) = child.try_wait().map_err(|err| {
            eval_failed(
                "codex-cli runner process wait failed",
                0,
                "runner_wait_failed",
                json!({"runner": "codex-cli", "error": err.to_string()}),
            )
        })? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            remove_trace_files(&[&stdout_path, &stderr_path, &last_message_path]);
            return Err(eval_failed(
                "codex-cli runner timed out",
                0,
                "runner_timeout",
                json!({
                    "runner": "codex-cli",
                    "timeout_ms": timeout.as_millis() as u64,
                    "workspace": workspace.display().to_string(),
                }),
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    let stdout_raw = fs::read_to_string(&stdout_path)
        .map_err(|err| io_failure("codex_trace_stdout_read", &stdout_path, err))?;
    let stderr_raw = fs::read_to_string(&stderr_path)
        .map_err(|err| io_failure("codex_trace_stderr_read", &stderr_path, err))?;
    let last_message = match fs::read_to_string(&last_message_path) {
        Ok(value) => Some(value),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(io_failure(
                "codex_last_message_read",
                &last_message_path,
                err,
            ));
        }
    };
    remove_trace_files(&[&stdout_path, &stderr_path, &last_message_path]);
    let trace = parse_codex_jsonl(&stdout_raw)?;
    Ok(CodexExecution {
        trace,
        last_message,
        stderr: stderr_raw,
        exit_code: status.code().unwrap_or(-1),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn codex_timeout() -> std::result::Result<Duration, CommandFailure> {
    let Some(raw) = std::env::var("LOOM_EVAL_CODEX_TIMEOUT_MS").ok() else {
        return Ok(Duration::from_millis(DEFAULT_CODEX_TIMEOUT_MS));
    };
    let millis = raw.parse::<u64>().map_err(|err| {
        eval_failed(
            "invalid LOOM_EVAL_CODEX_TIMEOUT_MS",
            0,
            "runner_timeout_invalid",
            json!({"value": raw, "error": err.to_string()}),
        )
    })?;
    if millis == 0 {
        return Err(eval_failed(
            "invalid LOOM_EVAL_CODEX_TIMEOUT_MS",
            0,
            "runner_timeout_invalid",
            json!({"value": raw, "error": "timeout must be greater than zero"}),
        ));
    }
    Ok(Duration::from_millis(millis))
}

fn remove_trace_files(paths: &[&PathBuf]) {
    for path in paths {
        let _ = fs::remove_file(path);
    }
}

fn parse_codex_jsonl(raw: &str) -> std::result::Result<CodexTrace, CommandFailure> {
    let mut trace = CodexTrace::default();
    let mut seen = false;
    for (index, line) in raw.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        seen = true;
        let value = serde_json::from_str::<Value>(trimmed).map_err(|err| {
            eval_failed(
                "codex-cli runner emitted unparseable JSONL",
                0,
                "runner_trace_unparseable",
                json!({
                    "runner": "codex-cli",
                    "line": index + 1,
                    "error": err.to_string(),
                }),
            )
        })?;
        collect_trace_value(&value, None, &mut trace);
    }
    if !seen {
        return Err(eval_failed(
            "codex-cli runner emitted no JSONL trace",
            0,
            "runner_trace_empty",
            json!({"runner": "codex-cli"}),
        ));
    }
    trace.commands.sort();
    trace.commands.dedup();
    Ok(trace)
}

fn collect_trace_value(value: &Value, key: Option<&str>, trace: &mut CodexTrace) {
    match value {
        Value::Object(map) => {
            for (child_key, child) in map {
                collect_trace_value(child, Some(child_key), trace);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_trace_value(item, key, trace);
            }
        }
        Value::String(text) => {
            if key.is_some_and(is_command_key) {
                trace.commands.push(text.clone());
            } else if key.is_some_and(is_output_key) {
                trace.output.push(text.clone());
            }
        }
        Value::Number(number) => {
            if key.is_some_and(is_token_key)
                && let Some(value) = number.as_u64()
            {
                trace.tokens = Some(trace.tokens.unwrap_or(0).saturating_add(value));
            }
        }
        _ => {}
    }
}

fn is_command_key(key: &str) -> bool {
    matches!(key, "command" | "cmd" | "exec_command" | "shell_command")
}

fn is_output_key(key: &str) -> bool {
    matches!(
        key,
        "content" | "message" | "text" | "output" | "final" | "final_answer" | "last_message"
    )
}

fn is_token_key(key: &str) -> bool {
    matches!(
        key,
        "tokens" | "total_tokens" | "input_tokens" | "output_tokens"
    )
}

fn task_prompt(plan: &EvalPlan, case: &HarnessTaskCase, variant: EvalVariant) -> String {
    let task = case.prompt_text().unwrap_or("Run the eval task.");
    let variant_instruction = match variant {
        EvalVariant::WithSkill => format!(
            "WITH_SKILL: Use the Loom skill named '{}' when it is relevant.\nSkill source:\n{}",
            plan.skill,
            plan.skill_source
                .as_deref()
                .unwrap_or("<skill source unavailable>")
        ),
        EvalVariant::WithoutSkill => format!(
            "WITHOUT_SKILL_BASELINE: Do not use the Loom skill named '{}'.",
            plan.skill
        ),
    };
    format!(
        "{variant_instruction}\nAgent under eval: {}\nWork only inside the current workspace.\nTask:\n{task}\nReturn a concise final answer.",
        plan.agent
    )
}

fn trigger_prompt(plan: &EvalPlan, prompt: &str) -> String {
    format!(
        "Decide whether the Loom skill named '{}' should trigger for the user request below. Return only JSON like {{\"trigger\": true}} or {{\"trigger\": false}}.\nSkill source:\n{}\nUser request:\n{}",
        plan.skill,
        plan.skill_source
            .as_deref()
            .unwrap_or("<skill source unavailable>"),
        prompt
    )
}

fn parse_trigger_decision(output: &str) -> std::result::Result<bool, CommandFailure> {
    for line in output.lines().rev() {
        if let Some(value) = parse_trigger_json(line) {
            return Ok(value);
        }
    }
    if let Some(value) = parse_trigger_json(output) {
        return Ok(value);
    }
    let lower = output.to_ascii_lowercase();
    if lower.contains("true") && !lower.contains("false") {
        return Ok(true);
    }
    if lower.contains("false") && !lower.contains("true") {
        return Ok(false);
    }
    Err(eval_failed(
        "codex-cli trigger output did not contain a trigger decision",
        0,
        "runner_trigger_unparseable",
        json!({"runner": "codex-cli"}),
    ))
}

fn parse_trigger_json(raw: &str) -> Option<bool> {
    let value = serde_json::from_str::<Value>(raw.trim()).ok()?;
    match value {
        Value::Bool(value) => Some(value),
        Value::Object(map) => map
            .get("trigger")
            .or_else(|| map.get("should_trigger"))
            .and_then(Value::as_bool),
        _ => None,
    }
}

struct WorkspaceSnapshot {
    files: BTreeMap<String, String>,
}

impl WorkspaceSnapshot {
    fn capture(root: &Path) -> std::result::Result<Self, CommandFailure> {
        let files = workspace_files(root)?;
        Ok(Self { files })
    }

    fn changed_files(&self, root: &Path) -> std::result::Result<Vec<String>, CommandFailure> {
        let after = workspace_files(root)?;
        let mut changed = Vec::new();
        for (path, before_digest) in &self.files {
            if after.get(path) != Some(before_digest) {
                changed.push(path.clone());
            }
        }
        for path in after.keys() {
            if !self.files.contains_key(path) {
                changed.push(path.clone());
            }
        }
        changed.sort();
        changed.dedup();
        Ok(changed)
    }
}

fn workspace_files(root: &Path) -> std::result::Result<BTreeMap<String, String>, CommandFailure> {
    let mut files = BTreeMap::new();
    if !root.is_dir() {
        return Ok(files);
    }
    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|err| {
            CommandFailure::new(
                ErrorCode::IoError,
                format!("eval workspace walk failed for '{}': {err}", root.display()),
            )
        })?;
        let path = entry.path();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|err| {
                CommandFailure::new(
                    ErrorCode::IoError,
                    format!(
                        "eval workspace path strip failed for '{}': {err}",
                        path.display()
                    ),
                )
            })?
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let bytes =
            fs::read(path).map_err(|err| io_failure("eval_workspace_file_read", path, err))?;
        files.insert(rel, digest_bytes(&bytes));
    }
    Ok(files)
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", to_hex(&hasher.finalize()))
}
