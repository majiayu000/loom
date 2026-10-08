use super::*;

fn exported_backup() -> (TestDir, PathBuf) {
    let source = TestDir::new("backup-recovery-source");
    let (output, env) = run_loom(source.path(), &["workspace", "init"]);
    assert!(output.status.success(), "init failed: {env}");
    write_file(&source.path().join("skills/demo/SKILL.md"), "# Demo\n");
    let (output, env) = run_loom(source.path(), &["backup", "export"]);
    assert!(output.status.success(), "export failed: {env}");
    let artifact = PathBuf::from(env["data"]["artifact"].as_str().expect("artifact"));
    (source, artifact)
}

#[test]
fn backup_restores_optional_untracked_target_cache() {
    let source = TestDir::new("backup-target-cache-source");
    assert!(
        run_loom(source.path(), &["workspace", "init"])
            .0
            .status
            .success()
    );
    let cache = Path::new("state/target-cache/sentinel.bin");
    let bytes = [0, 255, 1, 128, 10];
    write_file(
        &source.path().join(".git/info/exclude"),
        "/state/target-cache/\n",
    );
    fs::create_dir_all(source.path().join("state/target-cache/empty")).expect("create cache");
    fs::write(source.path().join(cache), bytes).expect("write cache");
    assert_eq!(
        git_text(source.path(), &["ls-files", "--", cache.to_str().unwrap()]),
        "",
        "cache sentinel must not be recoverable from the Git bundle"
    );

    for include_cache in [true, false] {
        let artifact = source
            .path()
            .join(format!("backups/cache-{include_cache}.tar"));
        let artifact_arg = artifact.to_string_lossy().into_owned();
        let mut args = vec!["backup", "export", "--output", &artifact_arg];
        if include_cache {
            args.push("--include-target-cache");
        }
        let (output, env) = run_loom(source.path(), &args);
        assert!(output.status.success(), "export failed: {env}");
        assert_eq!(env["data"]["target_cache_included"], include_cache);

        let destination = TestDir::new("backup-target-cache-destination");
        let root = destination.path().join("root");
        let (output, env) = run_loom(&root, &["backup", "restore", &artifact_arg]);
        assert!(output.status.success(), "restore failed: {env}");
        if include_cache {
            assert_eq!(fs::read(root.join(cache)).expect("restored cache"), bytes);
            assert!(root.join("state/target-cache/empty").is_dir());
        } else {
            assert!(!root.join("state/target-cache").exists());
        }
    }
}

#[test]
fn backup_restore_rejects_regular_file_target_cache_root() {
    let source = TestDir::new("backup-file-target-cache-source");
    let (output, env) = run_loom(source.path(), &["workspace", "init"]);
    assert!(output.status.success(), "init failed: {env}");
    let cache = source.path().join("state/target-cache");
    fs::write(&cache, b"cache root bytes\0\xff").expect("write cache root");

    let (output, env) = run_loom(
        source.path(),
        &["backup", "export", "--include-target-cache"],
    );
    assert!(output.status.success(), "export failed: {env}");
    assert_eq!(env["data"]["target_cache_included"], true);
    let artifact = env["data"]["artifact"].as_str().expect("artifact");
    assert_non_directory_target_cache_restore_rejected(Path::new(artifact));
    assert_eq!(fs::read(cache).unwrap(), b"cache root bytes\0\xff");
}

#[cfg(unix)]
#[test]
fn backup_restore_rejects_symlink_target_cache_root() {
    let (_source, artifact) = exported_backup();
    let mut archive = tar::Archive::new(fs::File::open(&artifact).unwrap());
    let mut builder = tar::Builder::new(Vec::new());
    let mut cache_path = None;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        let path = entry.path().unwrap().into_owned();
        if path.ends_with("registry/state/registry") {
            cache_path = Some(path.with_file_name("target-cache"));
        }
        builder.append(&entry.header().clone(), &mut entry).unwrap();
    }
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_mode(0o777);
    header.set_size(0);
    header.set_mtime(0);
    builder
        .append_link(
            &mut header,
            cache_path.expect("registry directory"),
            "registry",
        )
        .unwrap();
    fs::write(&artifact, builder.into_inner().unwrap()).unwrap();

    assert_non_directory_target_cache_restore_rejected(&artifact);
}

fn assert_non_directory_target_cache_restore_rejected(artifact: &Path) {
    let destination = TestDir::new("backup-invalid-target-cache-destination");
    let root = destination.path().join("root");

    let (output, env) = run_loom(&root, &["backup", "restore", artifact.to_str().unwrap()]);
    assert!(
        !output.status.success(),
        "non-directory cache root was accepted: {env}"
    );
    assert_eq!(env["error"]["code"], "STATE_CORRUPT");
    assert!(
        env["error"]["message"]
            .as_str()
            .unwrap()
            .contains("state/target-cache")
    );
    assert!(!root.exists(), "rejected restore activated the destination");
    assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
}

#[test]
fn backup_restore_rejects_existing_empty_and_scaffold_roots() {
    let (_source, artifact) = exported_backup();
    for scaffold in [false, true] {
        let destination = TestDir::new("backup-existing-root");
        if scaffold {
            write_file(
                &destination.path().join("nested/.gitkeep"),
                "keep scaffolding",
            );
        }
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(destination.path()).unwrap().ino()
        };
        let (output, env) = run_loom(
            destination.path(),
            &[
                "backup",
                "restore",
                artifact.to_str().unwrap(),
                "--force-empty-root",
            ],
        );
        assert!(
            !output.status.success(),
            "existing destination was accepted: {env}"
        );
        assert_eq!(env["error"]["code"], "ARG_INVALID");
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(destination.path()).unwrap().ino(), inode);
        }
        if scaffold {
            assert_eq!(
                fs::read_to_string(destination.path().join("nested/.gitkeep")).unwrap(),
                "keep scaffolding"
            );
        } else {
            assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
        }
    }
}

#[cfg(debug_assertions)]
mod activation_races {
    use super::*;
    use std::process::{Child, Output, Stdio};
    use std::time::{Duration, Instant};

    struct PausedRestore {
        child: Option<Child>,
        pause: TestDir,
    }

    impl PausedRestore {
        fn start(destination: &Path, artifact: &Path, point: &str, force: bool) -> Self {
            let pause = TestDir::new("backup-restore-pause");
            let mut command = Command::new(env!("CARGO_BIN_EXE_loom"));
            command
                .args(["--json", "--root"])
                .arg(destination)
                .args(["backup", "restore"])
                .arg(artifact)
                .env("LOOM_TEST_BACKUP_RESTORE_PAUSE_POINT", point)
                .env("LOOM_TEST_BACKUP_RESTORE_PAUSE_DIR", pause.path())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            if force {
                command.arg("--force-empty-root");
            }
            let child = command.spawn().expect("start restore");
            let mut restore = Self {
                child: Some(child),
                pause,
            };
            let deadline = Instant::now() + Duration::from_secs(10);
            while !restore.pause.path().join("ready").exists() {
                assert!(Instant::now() < deadline, "restore did not reach {point}");
                assert!(
                    restore
                        .child
                        .as_mut()
                        .unwrap()
                        .try_wait()
                        .expect("poll restore")
                        .is_none(),
                    "restore exited before {point}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            restore
        }

        fn release(&self) {
            fs::write(self.pause.path().join("release"), "continue").expect("release restore");
        }

        fn finish(mut self) -> (Output, Value) {
            self.release();
            let output = self
                .child
                .take()
                .unwrap()
                .wait_with_output()
                .expect("wait restore");
            let env = serde_json::from_slice(&output.stdout).expect("restore envelope");
            (output, env)
        }
    }

    impl Drop for PausedRestore {
        fn drop(&mut self) {
            if let Some(child) = self.child.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    #[test]
    fn backup_restore_preserves_files_written_after_clone() {
        let (_source, artifact) = exported_backup();
        let container = TestDir::new("backup-concurrent-write");
        let destination = container.path().join("destination");
        let restore = PausedRestore::start(&destination, &artifact, "before_activation", false);
        write_file(&destination.join("keep.txt"), "concurrent user data\n");
        let (output, env) = restore.finish();
        assert!(
            !output.status.success(),
            "restore unexpectedly succeeded: {env}"
        );
        assert_eq!(
            fs::read_to_string(destination.join("keep.txt")).unwrap(),
            "concurrent user data\n"
        );
        assert!(!destination.join(".git").exists());
        assert_eq!(
            fs::read_dir(container.path()).unwrap().count(),
            1,
            "failed staging must be cleaned"
        );
    }

    #[test]
    fn backup_restore_never_replaces_a_new_empty_destination() {
        let (_source, artifact) = exported_backup();
        for point in ["before_activation", "before_rename"] {
            let container = TestDir::new("backup-empty-destination-race");
            let destination = container.path().join("destination");
            let restore = PausedRestore::start(&destination, &artifact, point, false);
            assert!(!destination.exists());
            fs::create_dir(&destination).expect("concurrently create empty destination");
            let (output, env) = restore.finish();
            assert!(
                !output.status.success(),
                "restore replaced an empty directory: {env}"
            );
            assert!(destination.is_dir());
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        }
    }

    #[test]
    fn concurrent_backup_restores_have_one_winner() {
        let (_source, artifact) = exported_backup();
        let container = TestDir::new("backup-concurrent-restores");
        let destination = container.path().join("destination");
        let first = PausedRestore::start(&destination, &artifact, "before_rename", false);
        let second = PausedRestore::start(&destination, &artifact, "before_rename", false);
        first.release();
        second.release();
        let (first_output, first_env) = first.finish();
        let (second_output, second_env) = second.finish();
        assert_ne!(
            first_output.status.success(),
            second_output.status.success(),
            "exactly one restore must win: first={first_env}, second={second_env}"
        );
        assert_eq!(
            fs::read_to_string(destination.join("skills/demo/SKILL.md")).unwrap(),
            "# Demo\n"
        );
        assert_eq!(fs::read_dir(container.path()).unwrap().count(), 1);
    }

    #[test]
    fn backup_restore_preserves_new_files_and_symlinks() {
        let (_source, artifact) = exported_backup();
        for point in ["before_activation", "before_rename"] {
            for kind in ["file", "symlink"] {
                let container = TestDir::new("backup-entry-race");
                let destination = container.path().join("destination");
                let restore = PausedRestore::start(&destination, &artifact, point, true);
                if kind == "file" {
                    fs::write(&destination, "concurrent").unwrap();
                } else {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink("missing", &destination).unwrap();
                    #[cfg(windows)]
                    std::os::windows::fs::symlink_dir("missing", &destination).unwrap();
                }
                let (output, env) = restore.finish();
                assert!(!output.status.success(), "restore replaced {kind}: {env}");
                assert_eq!(env["error"]["code"], "IO_ERROR");
                if kind == "file" {
                    assert_eq!(fs::read_to_string(&destination).unwrap(), "concurrent");
                } else {
                    assert_eq!(
                        fs::read_link(&destination).unwrap(),
                        PathBuf::from("missing")
                    );
                }
            }
        }
    }
}
