use std::path::Path;

use anyhow::{Result, anyhow};

pub fn validate_git_url(raw: &str) -> Result<()> {
    let url = raw.trim();
    if url.is_empty() {
        return Err(anyhow!("git url must not be empty"));
    }
    if url != raw {
        return Err(anyhow!(
            "git url must not include leading or trailing whitespace"
        ));
    }
    if url.starts_with('-') {
        return Err(anyhow!("git url must not start with '-'"));
    }
    if url
        .chars()
        .any(|ch| ch.is_ascii_control() || ch.is_whitespace())
    {
        return Err(anyhow!(
            "git url must not contain whitespace or control characters"
        ));
    }
    if let Some((scheme, _rest)) = url.split_once("://") {
        return match scheme {
            "https" | "ssh" => Ok(()),
            _ => Err(anyhow!(
                "unsupported git url scheme '{}'; use https:// or ssh://",
                scheme
            )),
        };
    }
    let path = Path::new(url);
    if path.is_absolute() {
        return validate_local_git_remote_path(path);
    }
    validate_scp_like_git_url(url)
}

fn validate_local_git_remote_path(path: &Path) -> Result<()> {
    if !path.exists() {
        if path.extension().is_some_and(|extension| extension == "git") {
            return Ok(());
        }
        return Err(anyhow!(
            "local git remote path does not exist: {}",
            path.display()
        ));
    }
    if path.join(".git").is_dir() {
        return Ok(());
    }
    if path.join("HEAD").is_file() && path.join("objects").is_dir() {
        return Ok(());
    }
    Err(anyhow!(
        "local git remote path must point to a git repository: {}",
        path.display()
    ))
}

fn validate_scp_like_git_url(url: &str) -> Result<()> {
    let Some((user_host, path)) = url.split_once(':') else {
        return Err(anyhow!(
            "git url must use https://, ssh://, or git@host:owner/repo.git"
        ));
    };
    if user_host.is_empty() || path.is_empty() || path.starts_with(':') {
        return Err(anyhow!(
            "git url must use https://, ssh://, or git@host:owner/repo.git"
        ));
    }
    if user_host.contains('/') || user_host == "ext" {
        return Err(anyhow!("unsupported git url"));
    }
    let host = user_host
        .rsplit_once('@')
        .map_or(user_host, |(_, host)| host);
    if host.is_empty() || host == "ext" {
        return Err(anyhow!("scp-like git url must include a hostname"));
    }
    if host.starts_with('-') || host.starts_with('.') {
        return Err(anyhow!("scp-like git url contains an invalid hostname"));
    }
    if !host
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err(anyhow!("scp-like git url contains an invalid hostname"));
    }
    Ok(())
}
