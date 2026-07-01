use std::path::Path;

use anyhow::{Result, anyhow};

use crate::state::AppContext;

use super::{run_git, run_git_allow_failure};

#[derive(Debug, Clone, Copy, Default)]
pub struct DiffShortStat {
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

pub fn diff_has_changes_from_ref(ctx: &AppContext, reference: &str, path: &Path) -> Result<bool> {
    let path_str = path.to_string_lossy();
    let output = run_git_allow_failure(ctx, &["diff", "--quiet", reference, "--", &path_str])?;
    if output.status.success() {
        return Ok(false);
    }
    if output.status.code() == Some(1) {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(anyhow!(
        "git {:?} failed: {}",
        ["diff", "--quiet", reference, "--", &path_str],
        stderr
    ))
}

pub fn diff_shortstat_from_ref(
    ctx: &AppContext,
    reference: &str,
    path: &Path,
) -> Result<DiffShortStat> {
    let path_str = path.to_string_lossy();
    let output = run_git(ctx, &["diff", "--shortstat", reference, "--", &path_str])?;
    parse_diff_shortstat(&output)
}

pub fn diff_changed_paths_from_ref(
    ctx: &AppContext,
    reference: &str,
    path: &Path,
    limit: usize,
) -> Result<(Vec<String>, bool)> {
    let path_str = path.to_string_lossy();
    let output = run_git(ctx, &["diff", "--name-only", reference, "--", &path_str])?;
    let all_paths = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let truncated = all_paths.len() > limit;
    Ok((all_paths.into_iter().take(limit).collect(), truncated))
}

pub(crate) fn parse_diff_shortstat(raw: &str) -> Result<DiffShortStat> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(DiffShortStat::default());
    }

    let mut stat = DiffShortStat::default();
    for part in trimmed.split(',').map(str::trim) {
        if let Some(count) = parse_diff_count(part, "files changed")
            .or_else(|| parse_diff_count(part, "file changed"))
        {
            stat.files_changed = count;
        } else if let Some(count) = parse_diff_count(part, "insertions(+)")
            .or_else(|| parse_diff_count(part, "insertion(+)"))
        {
            stat.insertions = count;
        } else if let Some(count) =
            parse_diff_count(part, "deletions(-)").or_else(|| parse_diff_count(part, "deletion(-)"))
        {
            stat.deletions = count;
        } else {
            return Err(anyhow!("unexpected git diff --shortstat segment: {}", part));
        }
    }
    Ok(stat)
}

fn parse_diff_count(part: &str, suffix: &str) -> Option<u32> {
    let raw = part.strip_suffix(suffix)?.trim();
    raw.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_shortstat_handles_empty_and_partial_segments() {
        let empty = parse_diff_shortstat("").expect("empty stat");
        assert_eq!(empty.files_changed, 0);
        assert_eq!(empty.insertions, 0);
        assert_eq!(empty.deletions, 0);

        let insert_only =
            parse_diff_shortstat("1 file changed, 2 insertions(+)").expect("insert-only stat");
        assert_eq!(insert_only.files_changed, 1);
        assert_eq!(insert_only.insertions, 2);
        assert_eq!(insert_only.deletions, 0);
    }
}
