use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow};

use crate::state::AppContext;

#[derive(Debug, Clone, Copy)]
pub(crate) enum GitEnvMode {
    Normal,
    Restricted,
}

pub(crate) fn run_git_raw_in_with_env_and_input(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
) -> Result<Output> {
    run_git_raw_in_with_env_mode_and_input(repo_dir, envs, input, args, GitEnvMode::Normal)
}

pub(crate) fn run_git_raw_in_with_env_mode_and_input(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
    env_mode: GitEnvMode,
) -> Result<Output> {
    let mut command = Command::new("git");
    if matches!(env_mode, GitEnvMode::Restricted) {
        command.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0");
    }
    command
        .current_dir(repo_dir)
        .arg("-c")
        .arg("commit.gpgsign=false")
        .arg("-c")
        .arg("tag.gpgSign=false")
        .arg("-c")
        .arg("protocol.allow=never")
        .arg("-c")
        .arg("protocol.https.allow=always")
        .arg("-c")
        .arg("protocol.ssh.allow=always");
    if matches!(env_mode, GitEnvMode::Normal) {
        command.arg("-c").arg("protocol.file.allow=always");
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run git {:?}", args))?;
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().context("failed to open git stdin")?;
        stdin
            .write_all(bytes)
            .with_context(|| format!("failed to write git stdin for {:?}", args))?;
    }

    child
        .wait_with_output()
        .with_context(|| format!("failed to read git output for {:?}", args))
}

pub fn run_git(ctx: &AppContext, args: &[&str]) -> Result<String> {
    run_git_in(&ctx.root, args)
}

pub(crate) fn run_git_in(repo_dir: &Path, args: &[&str]) -> Result<String> {
    run_git_in_with_env(repo_dir, &[], args)
}

pub(crate) fn run_git_in_with_env(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> Result<String> {
    let output = run_git_allow_failure_in_with_env(repo_dir, envs, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn run_git_in_with_input(
    repo_dir: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<String> {
    let output = run_git_raw_in_with_env_and_input(repo_dir, &[], Some(input), args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn run_git_allow_failure(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    run_git_allow_failure_in(&ctx.root, args)
}

pub fn run_git_allow_failure_restricted(ctx: &AppContext, args: &[&str]) -> Result<Output> {
    run_git_raw_in_with_env_mode_and_input(&ctx.root, &[], None, args, GitEnvMode::Restricted)
}

pub(crate) fn run_git_allow_failure_in(repo_dir: &Path, args: &[&str]) -> Result<Output> {
    run_git_allow_failure_in_with_env(repo_dir, &[], args)
}

pub(crate) fn run_git_allow_failure_in_with_env(
    repo_dir: &Path,
    envs: &[(&str, &str)],
    args: &[&str],
) -> Result<Output> {
    run_git_raw_in_with_env_and_input(repo_dir, envs, None, args)
}
