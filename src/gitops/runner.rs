//! Single hardened entry point for every git subprocess spawned by loom.
//!
//! All git invocations in this crate must go through this module so the
//! hardening configuration (`commit.gpgsign=false`, `tag.gpgSign=false`,
//! `protocol.allow=never` plus an explicit transport whitelist) is applied
//! uniformly instead of being re-declared per call site.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, anyhow};

/// Whether a hardened git invocation may use the local `file` transport.
///
/// The hardened baseline pins `protocol.allow=never` and only re-enables
/// `https` and `ssh`. The `file` transport (local paths and bundles) stays
/// blocked unless a call site opts in explicitly because its workflow needs
/// to read local repositories or bundle files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileProtocol {
    /// Keep the `file` transport blocked by `protocol.allow=never`.
    Blocked,
    /// Explicit opt-in: add `protocol.file.allow=always` for workflows that
    /// must clone/fetch from local repositories or bundle files.
    Allowed,
}

const HARDENED_BASE_ARGS: &[&str] = &[
    "-c",
    "commit.gpgsign=false",
    "-c",
    "tag.gpgSign=false",
    "-c",
    "protocol.allow=never",
    "-c",
    "protocol.https.allow=always",
    "-c",
    "protocol.ssh.allow=always",
];

const FILE_PROTOCOL_OPT_IN_ARGS: &[&str] = &["-c", "protocol.file.allow=always"];

/// Hardening `-c` arguments shared by every git subprocess in this crate.
///
/// Exposed so call sites that cannot use [`hardened_git_command`] directly
/// (e.g. `tokio::process::Command`) still apply the exact same configuration.
pub(crate) fn hardened_config_args(
    file_protocol: FileProtocol,
) -> impl Iterator<Item = &'static str> {
    let opt_in = match file_protocol {
        FileProtocol::Blocked => &[][..],
        FileProtocol::Allowed => FILE_PROTOCOL_OPT_IN_ARGS,
    };
    HARDENED_BASE_ARGS.iter().chain(opt_in).copied()
}

/// Build a `git` command with the full hardening configuration but no
/// working directory, for invocations that operate on a remote locator only
/// (e.g. `git ls-remote <url>`).
pub(crate) fn hardened_git_command_no_dir(file_protocol: FileProtocol) -> Command {
    let mut command = Command::new("git");
    command.args(hardened_config_args(file_protocol));
    command
}

/// Build a hardened `git` command running in `repo_dir`.
pub(crate) fn hardened_git_command(repo_dir: &Path, file_protocol: FileProtocol) -> Command {
    let mut command = hardened_git_command_no_dir(file_protocol);
    command.current_dir(repo_dir);
    command
}

/// Run a hardened git command in `repo_dir`, returning stdout on success.
pub(crate) fn run_git_in_dir(
    repo_dir: &Path,
    file_protocol: FileProtocol,
    args: &[&str],
) -> Result<String> {
    let output = run_git_allow_failure_in_dir(repo_dir, file_protocol, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!("git {:?} failed: {}", args, stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run a hardened git command in `repo_dir`, returning the raw output even
/// when git exits with a non-zero status.
pub(crate) fn run_git_allow_failure_in_dir(
    repo_dir: &Path,
    file_protocol: FileProtocol,
    args: &[&str],
) -> Result<Output> {
    run_git_raw(repo_dir, file_protocol, &[], None, args)
}

pub(super) fn run_git_raw(
    repo_dir: &Path,
    file_protocol: FileProtocol,
    envs: &[(&str, &str)],
    input: Option<&[u8]>,
    args: &[&str],
) -> Result<Output> {
    let mut command = hardened_git_command(repo_dir, file_protocol);
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
