mod history;
mod history_impl;

mod commit;
mod exec;
mod index;
mod objects;
mod remote;
mod repo;
mod url;

pub use history::*;

pub use commit::{commit, commit_paths_if_changed, stage_path};
pub use exec::{run_git, run_git_allow_failure, run_git_allow_failure_restricted};
pub use index::{IndexSnapshot, restore_index, snapshot_index};
pub use objects::{diff_path, fsck};
pub use remote::{
    ahead_behind_main, ahead_behind_refs, fetch_origin_history_branch_if_present,
    fetch_origin_main_if_present, pull_rebase_main, push_main_with_tags,
    remote_exists, remote_tracking_history_exists, remote_tracking_main_exists, remote_url,
    set_remote_origin,
};
pub use repo::{
    checkout_path_from_ref, create_annotated_tag, ensure_repo_initialized,
    has_staged_changes_for_path, head, repo_is_initialized, resolve_ref, short_head,
};
pub use url::validate_git_url;

// Re-export internals needed by history_impl (accessed via `super::`)
pub(crate) use exec::run_git_in_with_env;
pub(crate) use objects::{TempFile, hash_object_bytes, hash_object_file, read_blob};
pub(crate) use repo::ensure_local_identity;

pub const HISTORY_BRANCH: &str = "loom-history";
const HISTORY_BRANCH_REF: &str = "refs/heads/loom-history";
const ORIGIN_HISTORY_BRANCH_REF: &str = "refs/remotes/origin/loom-history";
const HISTORY_SEGMENTS_DIR: &str = "pending_ops_history";
const HISTORY_ARCHIVES_DIR: &str = "pending_ops_archive";
const HISTORY_SNAPSHOT_FILE: &str = "pending_ops_snapshot.json";
const EMPTY_TREE_SHA: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const HISTORY_COMPACT_AFTER_SEGMENTS: usize = 8;
const HISTORY_RETAIN_RECENT_SEGMENTS: usize = 4;
const HISTORY_RETAIN_ARCHIVES: usize = 4;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppContext;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use uuid::Uuid;

    fn fresh_repo(label: &str) -> (AppContext, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("loom-gitops-{}-{}", label, Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp dir");
        for args in [
            ["init", "-q", "-b", "main"].as_slice(),
            ["config", "user.email", "test@example.com"].as_slice(),
            ["config", "user.name", "test"].as_slice(),
            ["config", "commit.gpgsign", "false"].as_slice(),
            ["config", "tag.gpgSign", "false"].as_slice(),
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }
        // Stage + commit a tracked file so `.git/index` exists and HEAD is real.
        // snapshot_index requires an existing index; flag-bearing tests need a
        // tracked path to attach skip-worktree / assume-unchanged to.
        fs::write(dir.join("base.txt"), "base\n").expect("write base");
        for args in [
            ["add", "base.txt"].as_slice(),
            ["commit", "-q", "-m", "init"].as_slice(),
        ] {
            let out = Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .expect("run git");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        }

        let ctx = AppContext::new(Some(dir.clone())).expect("build AppContext");
        (ctx, dir)
    }

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        git_ok_with_env(dir, args, &[])
    }

    fn git_ok_with_env(dir: &Path, args: &[&str], envs: &[(&str, &str)]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .envs(envs.iter().copied())
            .args(args)
            .output()
            .expect("run git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("git stdout utf8")
    }

    /// `ls-files -v` tag legend: H=cached, h=assume-unchanged, S=skip-worktree,
    /// s=skip-worktree+assume-unchanged.
    fn ls_files_v(dir: &Path) -> String {
        git_ok(dir, &["ls-files", "-v"])
    }

    #[test]
    fn snapshot_round_trip_preserves_intent_to_add() {
        let (ctx, dir) = fresh_repo("ita");

        fs::write(dir.join("ita.txt"), "stand-in").expect("write ita");
        git_ok(&dir, &["add", "-N", "--", "ita.txt"]);
        let before = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            before.lines().any(|l| l == " A ita.txt"),
            "expected IT-A marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        // Clobber the entry the way a stale rollback would.
        git_ok(&dir, &["update-index", "--force-remove", "ita.txt"]);
        let cleared = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            cleared.lines().any(|l| l == "?? ita.txt"),
            "force-remove must turn IT-A into untracked, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = git_ok(&dir, &["status", "--porcelain"]);
        assert!(
            restored.lines().any(|l| l == " A ita.txt"),
            "intent-to-add must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_respects_alternate_git_index() {
        let (ctx, dir) = fresh_repo("alternate-index");
        let default_index = dir.join(".git").join("index");
        let alternate_index = dir.join("alternate-index");
        fs::copy(&default_index, &alternate_index).expect("seed alternate index");
        let alternate_index = alternate_index.to_string_lossy().to_string();
        let envs = [("GIT_INDEX_FILE", alternate_index.as_str())];

        let default_before = fs::read(&default_index).expect("read default index");
        fs::write(dir.join("alt.txt"), "stand-in").expect("write alternate ita");
        git_ok_with_env(&dir, &["add", "-N", "--", "alt.txt"], &envs);
        let before = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            before.lines().any(|l| l == " A alt.txt"),
            "expected IT-A marker in alternate index pre-snapshot, got:\n{before}"
        );

        let snapshot = index::snapshot_index_with_env(&ctx, &envs).expect("snapshot alternate index");

        git_ok_with_env(&dir, &["update-index", "--force-remove", "alt.txt"], &envs);
        let cleared = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            cleared.lines().any(|l| l == "?? alt.txt"),
            "force-remove must clear alternate index entry, got:\n{cleared}"
        );

        index::restore_index_with_env(&ctx, &snapshot, &envs).expect("restore alternate index");

        let restored = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
        assert!(
            restored.lines().any(|l| l == " A alt.txt"),
            "alternate index IT-A marker must survive snapshot/restore, got:\n{restored}"
        );
        let default_after = fs::read(&default_index).expect("read default index after restore");
        assert_eq!(
            default_after, default_before,
            "snapshot/restore with GIT_INDEX_FILE must not overwrite the default index"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_preserves_skip_worktree_flag() {
        let (ctx, dir) = fresh_repo("skip-worktree");

        git_ok(&dir, &["update-index", "--skip-worktree", "base.txt"]);
        let before = ls_files_v(&dir);
        assert!(
            before.lines().any(|l| l == "S base.txt"),
            "expected skip-worktree marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        git_ok(&dir, &["update-index", "--no-skip-worktree", "base.txt"]);
        let cleared = ls_files_v(&dir);
        assert!(
            cleared.lines().any(|l| l == "H base.txt"),
            "skip-worktree should clear, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = ls_files_v(&dir);
        assert!(
            restored.lines().any(|l| l == "S base.txt"),
            "skip-worktree flag must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_round_trip_preserves_assume_unchanged_flag() {
        let (ctx, dir) = fresh_repo("assume-unchanged");

        git_ok(&dir, &["update-index", "--assume-unchanged", "base.txt"]);
        let before = ls_files_v(&dir);
        assert!(
            before.lines().any(|l| l == "h base.txt"),
            "expected assume-unchanged marker pre-snapshot, got:\n{before}"
        );

        let snapshot = snapshot_index(&ctx).expect("snapshot");

        git_ok(&dir, &["update-index", "--no-assume-unchanged", "base.txt"]);
        let cleared = ls_files_v(&dir);
        assert!(
            cleared.lines().any(|l| l == "H base.txt"),
            "assume-unchanged should clear, got:\n{cleared}"
        );

        restore_index(&ctx, &snapshot).expect("restore");

        let restored = ls_files_v(&dir);
        assert!(
            restored.lines().any(|l| l == "h base.txt"),
            "assume-unchanged flag must survive snapshot/restore round trip, got:\n{restored}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_snapshot_drop_removes_backup_file() {
        let (ctx, dir) = fresh_repo("drop-cleanup");

        let backup_path = {
            let snapshot = snapshot_index(&ctx).expect("snapshot");
            let path = snapshot.backup_path().to_path_buf();
            assert!(
                path.exists(),
                "backup must exist before drop: {}",
                path.display()
            );
            path
            // snapshot drops here
        };
        assert!(
            !backup_path.exists(),
            "backup must be removed by Drop: {}",
            backup_path.display()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn git_url_validation_accepts_https_and_ssh_forms() {
        validate_git_url("https://github.com/org/repo.git").expect("https accepted");
        validate_git_url("ssh://git@github.com/org/repo.git").expect("ssh accepted");
        validate_git_url("git@github.com:org/repo.git").expect("scp-like ssh accepted");
        validate_git_url("git@localhost:repo.git").expect("localhost scp-like ssh accepted");
        validate_git_url("git@github:org/repo.git").expect("ssh config alias accepted");
        validate_git_url("git@github_work:org/repo.git").expect("ssh config alias accepted");
    }

    #[test]
    fn git_url_validation_accepts_existing_local_git_repo_path() {
        let base = std::env::temp_dir().join(format!(
            "loom-local-remote-url-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repo = base.join("origin.git");
        fs::create_dir_all(&base).expect("base dir");
        let output = Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(&repo)
            .output()
            .expect("git init --bare");
        assert!(
            output.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        validate_git_url(repo.to_string_lossy().as_ref()).expect("local bare repo accepted");

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn git_url_validation_accepts_missing_local_git_path_for_later_creation() {
        let base = std::env::temp_dir().join(format!(
            "loom-missing-local-remote-url-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let repo = base.join("future.git");
        fs::create_dir_all(&base).expect("base dir");

        validate_git_url(repo.to_string_lossy().as_ref())
            .expect("missing .git path accepted for later creation");

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn git_url_validation_rejects_dangerous_protocols_and_options() {
        for url in [
            "ext::sh -c 'touch /tmp/pwned'",
            "file:///etc/passwd",
            "--upload-pack=sh",
            "git://github.com/org/repo.git",
            " https://github.com/org/repo.git",
            "git@ext:repo.git",
            "git@-github:org/repo.git",
            "git@.github:org/repo.git",
            "git@bad/host:org/repo.git",
            "git@bad=host:org/repo.git",
        ] {
            assert!(validate_git_url(url).is_err(), "{url} should be rejected");
        }
    }
}
