use std::fs;
use std::path::PathBuf;

use crate::cli::WatchArgs;
use crate::gitops;
use crate::state::AppContext;
use crate::types::ErrorCode;

use super::{
    collect_stable_watch_plan_with_wait, collect_watch_snapshot, ensure_watch_snapshot_unchanged,
};

struct Fixture {
    ctx: AppContext,
    file: PathBuf,
    args: WatchArgs,
}

impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("loom-watch-snapshot-{}", uuid::Uuid::new_v4()));
        let file = root.join("skills/demo/SKILL.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let ctx = AppContext::new(Some(root.clone())).unwrap();
        gitops::ensure_repo_initialized(&ctx).unwrap();
        fs::write(&file, "# demo\n\nv1\n").unwrap();
        gitops::run_git(&ctx, &["add", "skills/demo/SKILL.md"]).unwrap();
        gitops::run_git(&ctx, &["commit", "-m", "initial skill"]).unwrap();
        fs::write(&file, "# demo\n\nv2\n").unwrap();
        Self {
            ctx,
            file,
            args: WatchArgs {
                skill: Some("demo".to_string()),
                debounce_ms: 1,
                max_batch: 20,
                dry_run: false,
                once: true,
                max_cycles: None,
            },
        }
    }

    fn add_gitlink(&self) -> PathBuf {
        gitops::run_git(&self.ctx, &["checkout", "--", "skills/demo/SKILL.md"]).unwrap();
        let source = self.ctx.root.join("submodule-source");
        fs::create_dir(&source).unwrap();
        gitops::run_git_in_dir(&source, gitops::FileProtocol::Blocked, &["init"]).unwrap();
        let source_ctx = AppContext::new(Some(source.clone())).unwrap();
        gitops::ensure_repo_initialized(&source_ctx).unwrap();
        fs::write(source.join("content"), "unchanged\n").unwrap();
        gitops::run_git(&source_ctx, &["add", "content"]).unwrap();
        gitops::run_git(&source_ctx, &["commit", "-m", "initial submodule"]).unwrap();
        gitops::run_git_in_dir(
            &self.ctx.root,
            gitops::FileProtocol::Allowed,
            &[
                "submodule",
                "add",
                source.to_str().unwrap(),
                "skills/demo/module",
            ],
        )
        .unwrap();
        gitops::run_git(&self.ctx, &["commit", "-m", "add submodule"]).unwrap();
        let module = self.ctx.root.join("skills/demo/module");
        let module_ctx = AppContext::new(Some(module.clone())).unwrap();
        gitops::ensure_repo_initialized(&module_ctx).unwrap();
        advance_gitlink(&module);
        module
    }
}

fn advance_gitlink(module: &std::path::Path) {
    gitops::run_git_in_dir(
        module,
        gitops::FileProtocol::Blocked,
        &["commit", "--allow-empty", "-m", "advance submodule HEAD"],
    )
    .unwrap();
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.ctx.root);
    }
}

#[test]
fn same_path_edits_between_every_sample_are_not_stable() {
    let fixture = Fixture::new();
    let mut waits = 0;
    let result = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |duration| {
        assert_eq!(duration.as_millis(), 1);
        waits += 1;
        fs::write(&fixture.file, format!("# demo\n\nv{}\n", waits + 2)).unwrap();
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::CaptureConflict);
    assert_eq!(waits, 2);
}

#[test]
fn same_path_edit_restarts_the_quiet_period() {
    let fixture = Fixture::new();
    let mut waits = 0;
    let snapshot = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {
        waits += 1;
        if waits == 1 {
            fs::write(&fixture.file, "# demo\n\nv3\n").unwrap();
        }
    })
    .unwrap();
    assert_eq!(waits, 2);
    assert_eq!(
        snapshot,
        collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap()
    );
}

#[test]
fn quiet_edits_need_only_one_wait() {
    let fixture = Fixture::new();
    let mut waits = 0;
    let snapshot =
        collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| waits += 1).unwrap();
    assert_eq!(waits, 1);
    assert_eq!(snapshot.plan.path_count(), 1);
    ensure_watch_snapshot_unchanged(&fixture.ctx, &fixture.args, &snapshot).unwrap();
}

#[test]
fn locked_recheck_rejects_same_path_edit_after_debounce() {
    let fixture = Fixture::new();
    let snapshot =
        collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {}).unwrap();
    let _workspace = fixture.ctx.lock_workspace().unwrap();
    fs::write(&fixture.file, "# demo\n\nv3\n").unwrap();
    let error =
        ensure_watch_snapshot_unchanged(&fixture.ctx, &fixture.args, &snapshot).unwrap_err();
    assert_eq!(error.code, ErrorCode::CaptureConflict);
    assert!(error.message.contains("after autosave debounce"));
}

#[test]
fn gitlink_head_changes_between_every_sample_are_not_stable() {
    let fixture = Fixture::new();
    let module = fixture.add_gitlink();
    let mut waits = 0;
    let result = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {
        waits += 1;
        advance_gitlink(&module);
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::CaptureConflict);
    assert_eq!(waits, 2);
}

#[test]
fn quiet_gitlink_head_is_stable() {
    let fixture = Fixture::new();
    fixture.add_gitlink();
    let mut waits = 0;
    let snapshot =
        collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| waits += 1).unwrap();
    assert_eq!(waits, 1);
    assert_eq!(snapshot.plan.path_count(), 1);
    ensure_watch_snapshot_unchanged(&fixture.ctx, &fixture.args, &snapshot).unwrap();
}

#[test]
fn locked_recheck_rejects_gitlink_head_change_after_debounce() {
    let fixture = Fixture::new();
    let module = fixture.add_gitlink();
    let snapshot =
        collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {}).unwrap();
    let _workspace = fixture.ctx.lock_workspace().unwrap();
    advance_gitlink(&module);
    let error =
        ensure_watch_snapshot_unchanged(&fixture.ctx, &fixture.args, &snapshot).unwrap_err();
    assert_eq!(error.code, ErrorCode::CaptureConflict);
    assert!(error.message.contains("after autosave debounce"));
}

#[test]
fn gitlink_snapshot_failure_is_a_capture_conflict() {
    let fixture = Fixture::new();
    let module = fixture.add_gitlink();
    let plan = super::super::collect_watch_plan(&fixture.ctx, &fixture.args).unwrap();
    fs::write(module.join(".git"), "gitdir: missing\n").unwrap();
    let error = super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).unwrap_err();
    assert_eq!(error.code, ErrorCode::CaptureConflict);
}

#[test]
fn ordinary_directory_snapshots_do_not_track_the_parent_repository_head() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.ctx.root.join("skills/demo/plain")).unwrap();
    let mut plan = super::super::collect_watch_plan(&fixture.ctx, &fixture.args).unwrap();
    plan.skills[0].paths = vec!["skills/demo/plain".to_string()];
    let before = super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).unwrap();
    advance_gitlink(&fixture.ctx.root);
    let after = super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).unwrap();
    assert_eq!(before, after);
}

#[test]
fn zero_debounce_still_checks_the_snapshot_before_saving() {
    let mut fixture = Fixture::new();
    fixture.args.debounce_ms = 0;
    let snapshot = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {
        panic!("zero debounce must not wait")
    })
    .unwrap();
    fs::write(&fixture.file, "# demo\n\nv3\n").unwrap();
    assert_eq!(
        ensure_watch_snapshot_unchanged(&fixture.ctx, &fixture.args, &snapshot)
            .unwrap_err()
            .code,
        ErrorCode::CaptureConflict
    );
}

#[test]
fn untracked_same_path_content_changes_are_detected() {
    let fixture = Fixture::new();
    let path = fixture.ctx.root.join("skills/demo/new.bin");
    fs::write(&path, [0, 1, 2, 3]).unwrap();
    let first = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, [0, 1, 2, 4]).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    let second = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    assert_eq!(first.plan, second.plan);
    match (&first.entries[1], &second.entries[1]) {
        (
            super::watch_snapshot::WatchPathSnapshot::File { digest: before, .. },
            super::watch_snapshot::WatchPathSnapshot::File { digest: after, .. },
        ) => assert_ne!(
            before, after,
            "same size and mtime still require content identity"
        ),
        _ => panic!("expected binary file snapshots"),
    }
}

#[test]
fn deletions_are_stable_but_recreation_changes_the_snapshot() {
    let fixture = Fixture::new();
    fs::remove_file(&fixture.file).unwrap();
    let snapshot =
        collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {}).unwrap();
    assert!(matches!(
        snapshot.entries[0],
        super::watch_snapshot::WatchPathSnapshot::Missing
    ));
    fs::write(&fixture.file, "# demo\n\nv3\n").unwrap();
    assert_ne!(
        snapshot,
        collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap()
    );
}

#[test]
fn unrelated_and_ignored_paths_do_not_reset_debounce() {
    let fixture = Fixture::new();
    let mut waits = 0;
    let snapshot = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {
        waits += 1;
        fs::create_dir_all(fixture.ctx.root.join("skills/other")).unwrap();
        fs::write(fixture.ctx.root.join("skills/other/SKILL.md"), "other").unwrap();
        fs::write(fixture.ctx.root.join("skills/demo/edit.tmp"), "temporary").unwrap();
    })
    .unwrap();
    assert_eq!(waits, 1);
    assert_eq!(snapshot.plan.path_count(), 1);
}

#[cfg(unix)]
#[test]
fn symlink_snapshots_track_link_targets_without_reading_referents() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture.ctx.root.join("outside");
    fs::write(&outside, "outside v1").unwrap();
    let link = fixture.ctx.root.join("skills/demo/link");
    symlink(&outside, &link).unwrap();
    let first = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    fs::write(&outside, "outside v2").unwrap();
    assert_eq!(
        first,
        collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap()
    );
    fs::remove_file(&link).unwrap();
    symlink(fixture.ctx.root.join("missing-target"), &link).unwrap();
    assert_ne!(
        first,
        collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn snapshot_reader_rejects_symlinked_ancestors_and_special_files() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture.ctx.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("file"), "must not read").unwrap();
    let link = fixture.ctx.root.join("skills/demo/link");
    symlink(&outside, &link).unwrap();
    let mut plan = super::super::collect_watch_plan(&fixture.ctx, &fixture.args).unwrap();
    plan.skills[0].paths = vec!["skills/demo/link/file".to_string()];
    assert!(super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).is_err());
    fs::create_dir(outside.join("directory")).unwrap();
    plan.skills[0].paths = vec!["skills/demo/link/directory".to_string()];
    assert!(super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).is_err());
    let fifo = fixture.ctx.root.join("skills/demo/pipe");
    let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: the test-owned path is NUL-terminated and remains live.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    plan.skills[0].paths = vec!["skills/demo/pipe".to_string()];
    assert!(super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).is_err());
}

#[cfg(unix)]
#[test]
fn executable_mode_changes_reset_the_snapshot() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new();
    let before = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    let mode = fs::metadata(&fixture.file).unwrap().permissions().mode();
    fs::set_permissions(&fixture.file, fs::Permissions::from_mode(mode ^ 0o100)).unwrap();
    let after = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    assert_eq!(before.plan, after.plan);
    assert_ne!(before, after);
}

#[cfg(windows)]
#[test]
fn snapshot_reader_rejects_in_root_junction_ancestors() {
    let fixture = Fixture::new();
    let target = fixture.ctx.root.join("other");
    fs::create_dir(&target).unwrap();
    fs::write(target.join("file"), "must not read").unwrap();
    let junction = fixture.ctx.root.join("skills/demo/junction");
    let result = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&junction)
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success(), "junction setup failed: {result:?}");
    let mut plan = super::super::collect_watch_plan(&fixture.ctx, &fixture.args).unwrap();
    plan.skills[0].paths = vec!["skills/demo/junction/file".to_string()];
    assert!(super::watch_snapshot::snapshot_paths(&fixture.ctx, &plan).is_err());
    fs::remove_dir(junction).unwrap();
}

#[test]
fn oversized_observations_can_settle_within_the_batch_limit() {
    let mut fixture = Fixture::new();
    fixture.args.max_batch = 1;
    let extra = fixture.ctx.root.join("skills/demo/extra.md");
    fs::write(&extra, "extra").unwrap();
    let oversized = collect_watch_snapshot(&fixture.ctx, &fixture.args).unwrap();
    assert_eq!(oversized.plan.path_count(), 2);
    assert!(
        oversized.entries.is_empty(),
        "do not read an oversized batch"
    );
    let mut waits = 0;
    let settled = collect_stable_watch_plan_with_wait(&fixture.ctx, &fixture.args, |_| {
        waits += 1;
        if waits == 1 {
            fs::remove_file(&extra).unwrap();
        }
    })
    .unwrap();
    assert_eq!(waits, 2);
    assert_eq!(settled.plan.path_count(), 1);
    assert_eq!(settled.entries.len(), 1);
}
