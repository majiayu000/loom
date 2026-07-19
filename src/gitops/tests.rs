use super::*;
use crate::sha256::{Sha256, to_hex};
use crate::state::AppContext;
use std::process::Command;
use uuid::Uuid;

pub(super) fn fresh_repo(label: &str) -> (AppContext, std::path::PathBuf) {
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

fn file_sha256(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(&fs::read(path).expect("read digest input"));
    format!("sha256:{}", to_hex(&hasher.finalize()))
}

fn seed_owned_index_lock(ctx: &AppContext, prepared: &Path, lock: &Path) {
    let bytes = fs::read(prepared).expect("prepared index bytes");
    let claim = super::prepared_index_paths::prepared_index_aux_path(ctx, prepared, ".lock-claim")
        .expect("claim path");
    fs::hard_link(prepared, &claim).expect("durable index claim");
    crate::fs_util::write_atomic_bytes(prepared, &bytes).expect("detach prepared evidence");
    fs::hard_link(&claim, lock).expect("publish owned index lock");
}

fn assert_no_index_aux_paths(ctx: &AppContext, prepared: &Path) {
    for suffix in [
        ".lock-claim",
        ".lock-capture",
        ".lock-guard",
        ".lock-publish",
        ".lock-sentinel",
    ] {
        assert!(
            !super::prepared_index_paths::prepared_index_aux_path(ctx, prepared, suffix)
                .expect("auxiliary path")
                .exists(),
            "private index path remained after completion: {suffix}"
        );
    }
}

#[test]
fn prepared_index_install_rejects_tamper_before_active_mutation() {
    let (ctx, dir) = fresh_repo("prepared-index-tamper");
    let active_index = dir.join(".git/index");
    let original_index = fs::read(&active_index).expect("original index");
    let original_head = git_ok(&dir, &["rev-parse", "HEAD"]);
    let backup = dir.join("index-backup");
    fs::copy(&active_index, &backup).expect("index backup");
    fs::write(dir.join("base.txt"), "reviewed\n").expect("reviewed source");
    let prepared = dir.join("prepared-index");
    assert!(
        prepare_index_for_paths(&ctx, &backup, &prepared, &["base.txt"])
            .expect("prepare alternate index")
    );
    let expected = file_sha256(&prepared);

    fs::write(dir.join("tampered.txt"), "tampered\n").expect("tampered path");
    let prepared_env = prepared.to_str().expect("prepared path");
    git_ok_with_env(
        &dir,
        &["add", "--", "tampered.txt"],
        &[("GIT_INDEX_FILE", prepared_env)],
    );
    let error = install_prepared_index_with_guard(&ctx, &prepared, &|candidate| {
        if file_sha256(candidate) != expected {
            return Err(anyhow!("prepared index digest mismatch"));
        }
        Ok(())
    })
    .expect_err("tampered prepared index must fail closed");

    assert!(error.to_string().contains("prepared index digest mismatch"));
    assert_eq!(
        fs::read(&active_index).expect("active index"),
        original_index
    );
    assert_eq!(
        fs::read(dir.join(".git/index.lock")).expect("retained published lock"),
        fs::read(&prepared).expect("prepared evidence")
    );
    assert_eq!(git_ok(&dir, &["rev-parse", "HEAD"]), original_head);
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_install_preserves_preexisting_lock() {
    let (ctx, dir) = fresh_repo("prepared-index-existing-lock");
    let active_index = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active_index, &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    let owner_bytes = b"owned by another git process\n";
    fs::write(&lock, owner_bytes).expect("preexisting index lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("preexisting lock must block installation");

    assert_eq!(fs::read(&lock).expect("preserved lock"), owner_bytes);
    fs::remove_file(&lock).expect("remove test lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[cfg(target_os = "linux")]
#[test]
fn prepared_index_install_crosses_filesystems_without_overwriting_the_lock() {
    use std::os::unix::fs::MetadataExt;

    let (ctx, dir) = fresh_repo("prepared-index-cross-filesystem");
    let active_index = dir.join(".git/index");
    let prepared_root =
        Path::new("/dev/shm").join(format!("loom-prepared-index-{}", Uuid::new_v4()));
    fs::create_dir(&prepared_root).expect("create cross-filesystem artifact root");
    let prepared = prepared_root.join("prepared-index");
    fs::copy(&active_index, &prepared).expect("copy prepared index");
    assert_ne!(
        fs::metadata(&prepared).expect("prepared metadata").dev(),
        fs::metadata(&active_index).expect("active metadata").dev(),
        "test requires /dev/shm and the repository to use different filesystems"
    );
    let expected = fs::read(&prepared).expect("prepared bytes");

    install_prepared_index_with_guard(&ctx, &prepared, &|candidate| {
        assert_eq!(fs::read(candidate)?, expected);
        Ok(())
    })
    .expect("install cross-filesystem prepared index");

    assert_eq!(fs::read(&active_index).expect("installed index"), expected);
    assert!(!dir.join(".git/index.lock").exists());
    fs::remove_dir_all(&prepared_root).expect("remove prepared index root");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_install_preserves_byte_identical_foreign_lock() {
    let (ctx, dir) = fresh_repo("prepared-index-identical-foreign-lock");
    let active_index = dir.join(".git/index");
    let original_index = fs::read(&active_index).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active_index, &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    fs::copy(&prepared, &lock).expect("byte-identical foreign lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("byte-identical foreign lock must block installation");

    assert_eq!(fs::read(&lock).expect("preserved lock"), original_index);
    assert_eq!(
        fs::read(&active_index).expect("active index"),
        original_index
    );
    fs::remove_file(&lock).expect("remove test lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_install_rejects_a_mutating_guard_without_evidence_damage() {
    let (ctx, dir) = fresh_repo("prepared-index-mutating-guard");
    let active_index = dir.join(".git/index");
    let original_index = fs::read(&active_index).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active_index, &prepared).expect("prepared index");
    let prepared_bytes = fs::read(&prepared).expect("prepared evidence");

    install_prepared_index_with_guard(&ctx, &prepared, &|candidate| {
        fs::write(candidate, b"mutated lock\n")?;
        Ok(())
    })
    .expect_err("mutating guard must fail closed");

    assert_eq!(
        fs::read(&prepared).expect("prepared after guard"),
        prepared_bytes
    );
    assert_eq!(
        fs::read(&active_index).expect("active after guard"),
        original_index
    );
    assert_eq!(
        fs::read(dir.join(".git/index.lock")).expect("retained owned lock"),
        prepared_bytes
    );
    assert!(prepared_index_claim_exists(&ctx, &prepared).expect("inspect retained claim"));
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_install_crash_helper() {
    let crash_in_guard = std::env::var_os("LOOM_TEST_INDEX_INSTALL_CRASH").is_some();
    if !crash_in_guard && std::env::var_os("LOOM_TEST_PREPARED_INDEX_CRASH_POINT").is_none() {
        return;
    }
    let root = std::env::var_os("LOOM_TEST_INDEX_INSTALL_ROOT").expect("crash root");
    let prepared = std::env::var_os("LOOM_TEST_INDEX_INSTALL_PREPARED").expect("prepared index");
    let ctx = AppContext::new(Some(root.into())).expect("crash context");
    let _ = install_prepared_index_with_guard(&ctx, Path::new(&prepared), &|_| {
        if crash_in_guard {
            std::process::exit(92);
        }
        Ok(())
    });
    unreachable!("crash helper returned")
}

#[test]
fn prepared_index_publication_crash_leaves_an_exact_recoverable_lock() {
    let (ctx, dir) = fresh_repo("published-lock");
    let active_index = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active_index, &prepared).expect("prepared index");
    let status = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "gitops::tests::prepared_index_install_crash_helper",
            "--nocapture",
        ])
        .env("LOOM_TEST_INDEX_INSTALL_CRASH", "published-lock")
        .env("LOOM_TEST_INDEX_INSTALL_ROOT", &dir)
        .env("LOOM_TEST_INDEX_INSTALL_PREPARED", &prepared)
        .status()
        .expect("run crash helper");
    assert_eq!(status.code(), Some(92));
    let lock = dir.join(".git/index.lock");
    assert_eq!(
        fs::read(&lock).expect("published lock"),
        fs::read(&prepared).unwrap()
    );
    assert!(
        recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
            .expect("recover exact published lock")
    );
    assert!(!lock.exists());
    assert_no_index_aux_paths(&ctx, &prepared);
    assert_eq!(
        fs::read(&active_index).expect("installed index"),
        fs::read(&prepared).unwrap()
    );
    let prepared_bytes = fs::read(&prepared).expect("independent prepared evidence");
    fs::write(&active_index, b"later active mutation\n").expect("mutate active index inode");
    assert_eq!(
        fs::read(&prepared).expect("prepared after active mutation"),
        prepared_bytes
    );
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_post_rename_crashes_converge_without_stale_private_state() {
    for point in [
        "after_index_rename",
        "after_lock_capture",
        "after_claim_remove",
    ] {
        let (ctx, dir) = fresh_repo("post-rename-crash");
        let active_index = dir.join(".git/index");
        let original = fs::read(&active_index).expect("original index");
        let prepared = dir.join("prepared-index");
        fs::write(dir.join("base.txt"), format!("prepared at {point}\n"))
            .expect("edit tracked path");
        assert!(
            prepare_index_for_paths(&ctx, &active_index, &prepared, &["base.txt"])
                .expect("prepare changed index")
        );
        let expected = fs::read(&prepared).expect("prepared bytes");
        let status = Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "gitops::tests::prepared_index_install_crash_helper",
                "--nocapture",
            ])
            .env("LOOM_TEST_PREPARED_INDEX_CRASH_POINT", point)
            .env("LOOM_TEST_INDEX_INSTALL_ROOT", &dir)
            .env("LOOM_TEST_INDEX_INSTALL_PREPARED", &prepared)
            .status()
            .expect("run crash helper");
        assert_eq!(status.code(), Some(93), "crash point {point} did not fire");
        let after_crash = fs::read(&active_index).expect("index after crash");
        if point == "after_lock_capture" {
            assert_eq!(after_crash, original);
        } else {
            assert_eq!(after_crash, expected);
        }

        let recovered = recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
            .expect("recover post-rename crash");
        assert_eq!(recovered, point != "after_claim_remove");
        assert_eq!(fs::read(&active_index).expect("recovered index"), expected);
        assert!(!dir.join(".git/index.lock").exists());
        assert_no_index_aux_paths(&ctx, &prepared);
        fs::remove_dir_all(&dir).expect("remove test repository");
    }
}

#[test]
fn prepared_index_recovery_retains_owned_lock_after_mutating_guard() {
    for replacement in [false, true] {
        let (ctx, dir) = fresh_repo("prepared-index-recovery-guard");
        let active_index = dir.join(".git/index");
        let original_index = fs::read(&active_index).expect("active index");
        let prepared = dir.join("prepared-index");
        fs::copy(&active_index, &prepared).expect("prepared index");
        let prepared_bytes = fs::read(&prepared).expect("prepared evidence");
        let lock = dir.join(".git/index.lock");
        seed_owned_index_lock(&ctx, &prepared, &lock);
        let foreign = b"concurrently replaced foreign lock\n";

        recover_prepared_index_lock_with_guard(&ctx, &prepared, &|candidate| {
            if replacement {
                fs::remove_file(candidate)?;
            }
            fs::write(candidate, foreign)?;
            Ok(())
        })
        .expect_err("mutating recovery guard must fail closed");

        assert_eq!(
            fs::read(&prepared).expect("prepared after guard"),
            prepared_bytes
        );
        assert_eq!(
            fs::read(&active_index).expect("active after guard"),
            original_index
        );
        assert_eq!(
            fs::read(&lock).expect("retained owned lock"),
            prepared_bytes
        );
        assert!(prepared_index_claim_exists(&ctx, &prepared).expect("inspect retained claim"));
        fs::remove_dir_all(&dir).expect("remove test repository");
    }
}

#[test]
fn prepared_index_lock_recovery_preserves_nonmatching_lock() {
    let (ctx, dir) = fresh_repo("prepared-index-recovery-foreign-lock");
    let prepared = dir.join("prepared-index");
    fs::copy(dir.join(".git/index"), &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    let foreign = b"foreign lock bytes\n";
    fs::write(&lock, foreign).expect("foreign lock");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("foreign lock must not be adopted");

    assert_eq!(fs::read(&lock).expect("preserved foreign lock"), foreign);
    fs::remove_file(&lock).expect("remove test lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_lock_recovery_preserves_byte_identical_unclaimed_lock() {
    let (ctx, dir) = fresh_repo("prepared-index-recovery-identical-lock");
    let prepared = dir.join("prepared-index");
    fs::copy(dir.join(".git/index"), &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    fs::copy(&prepared, &lock).expect("byte-identical unclaimed lock");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("unclaimed lock must not be adopted by bytes alone");

    assert_eq!(
        fs::read(&lock).expect("preserved lock"),
        fs::read(&prepared).expect("prepared evidence")
    );
    fs::remove_file(&lock).expect("remove test lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_index_recovery_collision_preserves_both_foreign_entries() {
    let (ctx, dir) = fresh_repo("prepared-index-recovery-collision");
    let prepared = dir.join("prepared-index");
    fs::copy(dir.join(".git/index"), &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    let capture =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-capture")
            .expect("capture path");
    let public_foreign = b"new public foreign lock\n";
    let captured_foreign = b"captured foreign lock\n";
    fs::write(&lock, public_foreign).expect("public foreign lock");
    fs::write(&capture, captured_foreign).expect("captured foreign lock");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("foreign restoration collision must fail closed");

    assert_eq!(fs::read(&lock).expect("public lock"), public_foreign);
    assert_eq!(fs::read(&capture).expect("captured lock"), captured_foreign);
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[cfg(unix)]
#[test]
fn prepared_index_lock_recovery_rejects_an_exact_symlink() {
    let (ctx, dir) = fresh_repo("prepared-index-recovery-symlink-lock");
    let prepared = dir.join("prepared-index");
    fs::copy(dir.join(".git/index"), &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    std::os::unix::fs::symlink(&prepared, &lock).expect("symlink lock");
    let active = fs::read(dir.join(".git/index")).expect("active index");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("symlink lock must not be adopted");

    assert!(
        fs::symlink_metadata(&lock)
            .expect("preserved lock")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read(dir.join(".git/index")).expect("active index"),
        active
    );
    fs::remove_file(&lock).expect("remove test lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn prepared_commit_ignores_late_worktree_tamper_without_moving_head_or_index() {
    let (ctx, dir) = fresh_repo("prepared-commit-worktree-tamper");
    let active_index = dir.join(".git/index");
    let original_index = fs::read(&active_index).expect("original index");
    let original_head = git_ok(&dir, &["rev-parse", "HEAD"]);
    let backup = dir.join("index-backup");
    fs::copy(&active_index, &backup).expect("index backup");
    fs::write(dir.join("base.txt"), "reviewed\n").expect("reviewed source");
    let prepared = dir.join("prepared-index");
    assert!(
        prepare_index_for_paths(&ctx, &backup, &prepared, &["base.txt"])
            .expect("prepare alternate index")
    );
    fs::write(dir.join("base.txt"), "tampered-after-index\n").expect("late worktree tamper");

    let commit_index = dir.join("prepared-commit-index");
    let commit = create_prepared_commit(
        &ctx,
        &prepared,
        &commit_index,
        &["base.txt"],
        original_head.trim(),
        "prepared source",
    )
    .expect("create prepared commit object");
    assert_eq!(
        git_ok(&dir, &["show", &format!("{commit}:base.txt")]),
        "reviewed\n"
    );
    let error = install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        if fs::read_to_string(dir.join("base.txt"))? != "reviewed\n" {
            return Err(anyhow!("live source changed after preparation"));
        }
        Ok(())
    })
    .expect_err("late source drift must fail before index installation");

    assert!(error.to_string().contains("live source changed"));
    assert_eq!(
        fs::read(&active_index).expect("active index"),
        original_index
    );
    assert_eq!(git_ok(&dir, &["rev-parse", "HEAD"]), original_head);
    fs::remove_dir_all(&dir).expect("remove test repository");
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

    let snapshot = snapshot_index_with_env(&ctx, &envs).expect("snapshot alternate index");

    git_ok_with_env(&dir, &["update-index", "--force-remove", "alt.txt"], &envs);
    let cleared = git_ok_with_env(&dir, &["status", "--porcelain"], &envs);
    assert!(
        cleared.lines().any(|l| l == "?? alt.txt"),
        "force-remove must clear alternate index entry, got:\n{cleared}"
    );

    restore_index_with_env(&ctx, &snapshot, &envs).expect("restore alternate index");

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

#[test]
fn origin_url_validation_uses_instead_of_expanded_effective_url() {
    let (ctx, dir) = fresh_repo("instead-of-url");
    git_ok(
        &dir,
        &[
            "config",
            "--local",
            "url.git://blocked.example/.insteadOf",
            "https://allowed.example/",
        ],
    );
    git_ok(
        &dir,
        &[
            "remote",
            "add",
            "origin",
            "https://allowed.example/org/repo.git",
        ],
    );

    assert_eq!(
        remote_url(&ctx)
            .expect("read effective remote url")
            .as_deref(),
        Some("git://blocked.example/org/repo.git")
    );
    let err = ensure_origin_remote_url_allowed(&ctx).expect_err("git protocol must be rejected");
    assert!(err.to_string().contains("unsupported git url scheme 'git'"));

    let _ = fs::remove_dir_all(&dir);
}
