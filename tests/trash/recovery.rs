use super::*;

const PAYLOAD: &[u8] = b"uncommitted recovery evidence\0\xff\n";

fn trashed_skill() -> (TestDir, String) {
    let root = TestDir::new("trash-payload-rollback");
    write_skill(root.path(), "demo", "# Demo\n\nv1\n");
    assert_success(&save_skill(root.path(), "demo").0, "save");
    let (output, env) = run_loom(root.path(), &["skill", "trash", "add", "demo"]);
    assert_success(&output, &format!("trash add: {env}"));
    let trash_id = env["data"]["trash_id"]
        .as_str()
        .expect("trash id")
        .to_owned();
    fs::write(
        root.path()
            .join("trash")
            .join(&trash_id)
            .join("skill/uncommitted.bin"),
        PAYLOAD,
    )
    .expect("write uncommitted payload");
    (root, trash_id)
}

fn command_args<'a>(operation: &'a str, trash_id: &'a str) -> Vec<&'a str> {
    if operation == "restore" {
        vec!["skill", "trash", "restore", "demo", "--trash-id", trash_id]
    } else {
        vec!["skill", "trash", "purge", trash_id]
    }
}

fn assert_retained_payload(env: &Value, step: &str, root: &Path, metadata: &[u8]) {
    let error = env["error"]["details"]["rollback_errors"]
        .as_array()
        .expect("rollback errors")
        .iter()
        .find(|error| error["step"] == step)
        .unwrap_or_else(|| panic!("missing {step} rollback error: {env}"));
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| !message.is_empty())
    );
    let backup = Path::new(error["backup_path"].as_str().expect("retained backup path"));
    assert!(backup.starts_with(root.join("state/backups")));
    assert_eq!(
        fs::read(backup.join("skill/uncommitted.bin")).expect("retained payload"),
        PAYLOAD
    );
    assert_eq!(
        fs::read(backup.join("metadata.json")).expect("retained metadata"),
        metadata
    );
}

fn fails_with_retained_backup(operation: &str) {
    let (root, trash_id) = trashed_skill();
    let entry = root.path().join("trash").join(&trash_id);
    let metadata = fs::read(entry.join("metadata.json")).unwrap();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[
            ("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint"),
            ("LOOM_ROLLBACK_FAULT_INJECT", "restore_trash_payload"),
        ],
        &command_args(operation, &trash_id),
    );
    assert!(
        !output.status.success(),
        "faulted {operation} succeeded: {env}"
    );
    assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    if operation == "restore" {
        assert_eq!(
            fs::read(root.path().join("skills/demo/uncommitted.bin")).expect("live recovery copy"),
            PAYLOAD,
            "the live copy must survive a failed trash payload rollback"
        );
    }
}

#[test]
fn trash_restore_retains_backup_when_payload_rollback_fails() {
    fails_with_retained_backup("restore");
}

#[test]
fn trash_purge_retains_backup_when_payload_rollback_fails() {
    fails_with_retained_backup("purge");
}

#[test]
fn trash_restore_reports_retained_live_copy_and_backup() {
    let (root, trash_id) = trashed_skill();
    let entry = root.path().join("trash").join(&trash_id);
    let metadata = fs::read(entry.join("metadata.json")).unwrap();
    let (output, env) = run_loom_with_env(
        root.path(),
        &[("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint")],
        &command_args("restore", &trash_id),
    );
    assert!(!output.status.success(), "faulted restore succeeded: {env}");
    assert_retained_payload(&env, "remove_restored_skill", root.path(), &metadata);
    assert_eq!(
        fs::read(entry.join("skill/uncommitted.bin")).unwrap(),
        PAYLOAD
    );
    assert_eq!(
        fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
        PAYLOAD
    );
}

#[test]
fn trash_rollback_retains_live_copy_and_cleans_only_completed_purge_recovery() {
    for operation in ["restore", "purge"] {
        let (root, trash_id) = trashed_skill();
        let entry = root.path().join("trash").join(&trash_id);
        let metadata = fs::read(entry.join("metadata.json")).unwrap();
        let operations_before = operations_log(root.path());
        let head_before = git_success(root.path(), &["rev-parse", "HEAD"]);
        let (output, env) = run_loom_with_env(
            root.path(),
            &[("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint")],
            &command_args(operation, &trash_id),
        );
        assert!(
            !output.status.success(),
            "faulted {operation} succeeded: {env}"
        );
        assert_eq!(
            fs::read(entry.join("skill/uncommitted.bin")).unwrap(),
            PAYLOAD
        );
        assert_eq!(fs::read(entry.join("metadata.json")).unwrap(), metadata);
        if operation == "restore" {
            assert_retained_payload(&env, "remove_restored_skill", root.path(), &metadata);
            assert_eq!(
                fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
                PAYLOAD
            );
        } else {
            assert!(!root.path().join("skills/demo").exists());
        }
        assert_eq!(operations_log(root.path()), operations_before);
        assert_eq!(
            git_success(root.path(), &["rev-parse", "HEAD"]),
            head_before
        );
        if operation == "purge" {
            for entry in walkdir::WalkDir::new(root.path().join("state/backups")) {
                let Ok(entry) = entry else { continue };
                let name = entry.file_name().to_string_lossy();
                assert!(
                    !name.starts_with("trash-restore-") && !name.starts_with("trash-purge-"),
                    "successfully restored backup was not cleaned: {}",
                    entry.path().display()
                );
            }
            assert!(
                env["error"]["details"]["rollback_errors"].is_null(),
                "unexpected rollback error: {env}"
            );
        }
    }
}

#[test]
fn successful_restore_and_purge_clean_their_temporary_backups() {
    for operation in ["restore", "purge"] {
        let (root, trash_id) = trashed_skill();
        let before = operations_log(root.path());
        let head = git_success(root.path(), &["rev-parse", "HEAD"]);
        let (output, env) = run_loom(root.path(), &command_args(operation, &trash_id));
        assert!(output.status.success(), "{operation} failed: {env}");
        assert!(!root.path().join("trash").join(trash_id).exists());
        assert_ne!(operations_log(root.path()), before);
        assert_ne!(git_success(root.path(), &["rev-parse", "HEAD"]), head);
        for entry in walkdir::WalkDir::new(root.path().join("state/backups")) {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name().to_string_lossy();
            assert!(
                !name.starts_with("trash_restore-") && !name.starts_with("trash_purge-"),
                "temporary backup or metadata capture survives success: {}",
                entry.path().display()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn trash_rollback_preserves_recreated_entry_and_live_edits() {
    use std::os::unix::fs::PermissionsExt;
    for operation in ["restore", "purge"] {
        let (root, trash_id) = trashed_skill();
        let entry = root.path().join("trash").join(&trash_id);
        let metadata = fs::read(entry.join("metadata.json")).unwrap();
        let hook = root.path().join(".git/hooks/pre-commit");
        write_file(
            &hook,
            &format!(
                "#!/bin/sh\nmkdir -p 'trash/{trash_id}'\nprintf newcomer > 'trash/{trash_id}/concurrent.txt'\nif [ -d skills/demo ]; then printf edited > skills/demo/uncommitted.bin; fi\nexit 1\n"
            ),
        );
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        let (output, env) = run_loom(root.path(), &command_args(operation, &trash_id));
        assert!(!output.status.success(), "hook failure ignored: {env}");
        assert_eq!(env["error"]["code"], "GIT_ERROR");
        assert_eq!(
            fs::read(entry.join("concurrent.txt")).expect("recreated entry preserved"),
            b"newcomer"
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
        if operation == "restore" {
            assert_eq!(
                fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
                b"edited"
            );
        }
    }
}

#[cfg(debug_assertions)]
mod races {
    use super::*;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    fn paused(
        operation: &str,
        point: &str,
        fail: bool,
        mutate: impl FnOnce(&Path, &Path),
    ) -> (TestDir, String, Value, Vec<u8>) {
        let (root, trash_id) = trashed_skill();
        let entry = root.path().join("trash").join(&trash_id);
        let metadata = fs::read(entry.join("metadata.json")).unwrap();
        let pause = TestDir::new("trash-race-pause");
        let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
        command
            .args(["--json", "--root"])
            .arg(root.path())
            .args(command_args(operation, &trash_id))
            .env("LOOM_TEST_TRASH_PAUSE_POINT", point)
            .env("LOOM_TEST_TRASH_PAUSE_DIR", pause.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if fail {
            command.env("LOOM_TEST_TRASH_FAIL_POINT", point);
        }
        if point == "before_rollback_restore" {
            command.env("LOOM_FAULT_INJECT", "record_v3_operation_after_checkpoint");
        }
        let mut child = command.spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pause.path().join("ready").exists() {
            if Instant::now() >= deadline || child.try_wait().unwrap().is_some() {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "did not reach {point}: {} {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        mutate(root.path(), &entry);
        fs::write(pause.path().join("release"), "continue").unwrap();
        let output = child.wait_with_output().unwrap();
        let env: Value = serde_json::from_slice(&output.stdout).unwrap();
        if env["error"]["code"] == "IO_ERROR" {
            assert_eq!(output.status.code(), Some(5));
        }
        assert!(
            !output.status.success(),
            "race unexpectedly succeeded: {env}"
        );
        (root, trash_id, env, metadata)
    }

    #[test]
    fn restore_activation_preserves_new_directory_file_and_symlink() {
        for kind in ["directory", "file", "symlink"] {
            let (root, trash_id, env, _) =
                paused("restore", "before_restore_activation", false, |root, _| {
                    let live = root.join("skills/demo");
                    match kind {
                        "directory" => fs::create_dir(&live).unwrap(),
                        "file" => fs::write(&live, b"newcomer").unwrap(),
                        _ => create_dir_symlink(Path::new("missing"), &live),
                    }
                });
            assert_eq!(env["error"]["code"], "IO_ERROR");
            let live = root.path().join("skills/demo");
            match kind {
                "directory" => assert_eq!(fs::read_dir(&live).unwrap().count(), 0),
                "file" => assert_eq!(fs::read(&live).unwrap(), b"newcomer"),
                _ => assert_eq!(fs::read_link(&live).unwrap(), PathBuf::from("missing")),
            }
            assert_eq!(
                fs::read(
                    root.path()
                        .join("trash")
                        .join(trash_id)
                        .join("skill/uncommitted.bin")
                )
                .unwrap(),
                PAYLOAD
            );
        }
    }

    #[test]
    fn restore_cleanup_preserves_unexpected_entry_content_and_live_edits() {
        let (root, trash_id, env, metadata) =
            paused("restore", "before_restore_cleanup", false, |root, entry| {
                fs::write(entry.join("concurrent.txt"), b"newcomer").unwrap();
                fs::write(root.join("skills/demo/uncommitted.bin"), b"live edit").unwrap();
            });
        assert_eq!(env["error"]["code"], "IO_ERROR");
        assert_eq!(
            fs::read(
                root.path()
                    .join("trash")
                    .join(trash_id)
                    .join("concurrent.txt")
            )
            .unwrap(),
            b"newcomer"
        );
        assert_eq!(
            fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
            b"live edit"
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }

    const REPLACEMENT_METADATA: &[u8] = b"concurrent metadata\0\xff\n";

    fn retained_metadata(env: &Value) -> PathBuf {
        PathBuf::from(
            env["error"]["details"]["rollback_errors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|error| error["step"] == "preserve_trash_metadata")
                .unwrap_or_else(|| panic!("missing retained metadata: {env}"))["path"]
                .as_str()
                .unwrap(),
        )
    }

    fn metadata_backup(root: &Path) -> PathBuf {
        walkdir::WalkDir::new(root.join("state/backups"))
            .into_iter()
            .filter_map(Result::ok)
            .find(|entry| {
                entry.file_type().is_dir()
                    && entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("trash_restore-")
            })
            .expect("independent trash backup")
            .into_path()
    }

    fn replaced_metadata_survives(with_new_file: bool) {
        let (root, trash_id, env, metadata) =
            paused("restore", "before_restore_cleanup", false, |_, entry| {
                let replacement = entry.join("replacement.tmp");
                fs::write(&replacement, REPLACEMENT_METADATA).unwrap();
                fs::rename(&replacement, entry.join("metadata.json")).unwrap();
                if with_new_file {
                    fs::write(entry.join("concurrent.txt"), b"newcomer").unwrap();
                }
            });
        assert_eq!(env["error"]["code"], "IO_ERROR");
        assert_eq!(
            fs::read(retained_metadata(&env)).unwrap(),
            REPLACEMENT_METADATA
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
        assert_eq!(
            fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
            PAYLOAD
        );
        let entry = root.path().join("trash").join(trash_id);
        assert!(entry.is_dir());
        if with_new_file {
            assert_eq!(fs::read(entry.join("concurrent.txt")).unwrap(), b"newcomer");
        }
    }

    #[test]
    fn restore_cleanup_preserves_replaced_metadata() {
        replaced_metadata_survives(false);
    }

    #[test]
    fn restore_cleanup_preserves_replaced_metadata_and_new_file() {
        replaced_metadata_survives(true);
    }

    #[test]
    fn restore_cleanup_preserves_metadata_capture_collision() {
        let (root, trash_id, env, metadata) =
            paused("restore", "before_restore_cleanup", false, |root, _| {
                fs::write(
                    metadata_backup(root).with_extension("metadata-recovery"),
                    b"other owner",
                )
                .unwrap();
            });
        assert_eq!(env["error"]["code"], "IO_ERROR");
        assert_eq!(
            fs::read(metadata_backup(root.path()).with_extension("metadata-recovery")).unwrap(),
            b"other owner"
        );
        assert_eq!(
            fs::read(
                root.path()
                    .join("trash")
                    .join(trash_id)
                    .join("metadata.json")
            )
            .unwrap(),
            metadata
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }

    #[test]
    fn restore_cleanup_preserves_metadata_recreated_after_capture() {
        let (root, trash_id, env, metadata) = paused(
            "restore",
            "after_restore_metadata_capture",
            false,
            |_, entry| {
                fs::write(entry.join("metadata.json"), REPLACEMENT_METADATA).unwrap();
            },
        );
        assert_eq!(env["error"]["code"], "IO_ERROR");
        assert_eq!(
            fs::read(
                root.path()
                    .join("trash")
                    .join(trash_id)
                    .join("metadata.json")
            )
            .unwrap(),
            REPLACEMENT_METADATA
        );
        assert_eq!(fs::read(retained_metadata(&env)).unwrap(), metadata);
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }

    #[test]
    fn restore_cleanup_capture_failure_preserves_primary_error_and_snapshot() {
        let (root, _, env, metadata) =
            paused("restore", "after_restore_metadata_capture", true, |_, _| {});
        assert_eq!(env["error"]["code"], "IO_ERROR");
        assert!(
            env["error"]["message"]
                .as_str()
                .unwrap()
                .contains("fault injected at after_restore_metadata_capture")
        );
        assert_eq!(fs::read(retained_metadata(&env)).unwrap(), metadata);
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }

    #[test]
    fn metadata_recovery_retains_snapshot_when_live_copy_is_absent() {
        let (root, trash_id, env, metadata) =
            paused("restore", "before_rollback_restore", false, |root, _| {
                fs::remove_dir_all(root.join("skills/demo")).unwrap();
            });
        assert_eq!(env["error"]["code"], "STATE_CORRUPT");
        assert_eq!(fs::read(retained_metadata(&env)).unwrap(), metadata);
        assert_retained_payload(&env, "preserve_trash_metadata", root.path(), &metadata);
        assert_eq!(
            fs::read(
                root.path()
                    .join("trash")
                    .join(trash_id)
                    .join("metadata.json")
            )
            .unwrap(),
            metadata
        );
    }

    #[test]
    fn rollback_preserves_entry_created_after_forward_cleanup() {
        let (root, trash_id, env, metadata) = paused(
            "restore",
            "before_rollback_restore",
            false,
            |root, entry| {
                fs::create_dir(entry).unwrap();
                fs::write(entry.join("concurrent.txt"), b"new entry").unwrap();
                fs::write(root.join("skills/demo/uncommitted.bin"), b"live edit").unwrap();
            },
        );
        assert_eq!(
            fs::read(
                root.path()
                    .join("trash")
                    .join(trash_id)
                    .join("concurrent.txt")
            )
            .unwrap(),
            b"new entry"
        );
        assert_eq!(
            fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
            b"live edit"
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }

    #[cfg(unix)]
    #[test]
    fn restore_cleanup_preserves_metadata_symlink_even_when_bytes_match() {
        for kind in ["same", "changed", "dangling"] {
            let (root, _, env, metadata) =
                paused("restore", "before_restore_cleanup", false, |root, entry| {
                    let current = entry.join("metadata.json");
                    let bytes = fs::read(&current).unwrap();
                    let referent = root.join("concurrent-metadata.json");
                    match kind {
                        "same" => fs::write(&referent, &bytes).unwrap(),
                        "changed" => fs::write(&referent, b"concurrent metadata").unwrap(),
                        _ => {}
                    }
                    fs::remove_file(&current).unwrap();
                    std::os::unix::fs::symlink(&referent, current).unwrap();
                });
            assert_eq!(env["error"]["code"], "IO_ERROR");
            let captured = retained_metadata(&env);
            let referent = root.path().join("concurrent-metadata.json");
            assert_eq!(fs::read_link(&captured).unwrap(), referent);
            match kind {
                "same" => assert_eq!(fs::read(&referent).unwrap(), metadata),
                "changed" => assert_eq!(fs::read(&referent).unwrap(), b"concurrent metadata"),
                _ => assert!(!referent.exists()),
            }
            assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
            assert_eq!(
                fs::read(root.path().join("skills/demo/uncommitted.bin")).unwrap(),
                PAYLOAD
            );
        }
    }

    #[test]
    fn failed_purge_preserves_surviving_entry_bytes_and_snapshot() {
        let (root, trash_id, env, metadata) =
            paused("purge", "before_purge_remove", true, |_, entry| {
                fs::write(entry.join("concurrent.txt"), b"survivor").unwrap();
                fs::write(
                    entry.join("skill/uncommitted.bin"),
                    b"changed after snapshot",
                )
                .unwrap();
            });
        assert_eq!(env["error"]["code"], "IO_ERROR");
        let entry = root.path().join("trash").join(trash_id);
        assert_eq!(fs::read(entry.join("concurrent.txt")).unwrap(), b"survivor");
        assert_eq!(
            fs::read(entry.join("skill/uncommitted.bin")).unwrap(),
            b"changed after snapshot"
        );
        assert_retained_payload(&env, "restore_trash_payload", root.path(), &metadata);
    }
}
