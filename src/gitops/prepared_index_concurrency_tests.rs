use std::fs;

use super::*;

#[test]
fn guard_replacement_preserves_a_byte_identical_foreign_lock() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-guard-lock-replacement");
    let active_index = dir.join(".git/index");
    let original_index = fs::read(&active_index).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::write(dir.join("base.txt"), "prepared content\n").expect("change tracked source");
    assert!(
        prepare_index_for_paths(&ctx, &active_index, &prepared, &["base.txt"])
            .expect("prepare distinct index")
    );
    assert_ne!(
        fs::read(&prepared).expect("prepared bytes"),
        original_index,
        "test requires distinguishable active and prepared indexes"
    );
    let lock = dir.join(".git/index.lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        fs::remove_file(&lock)?;
        fs::copy(&prepared, &lock)?;
        Ok(())
    })
    .expect_err("foreign replacement must block publication");

    assert_eq!(
        fs::read(&active_index).expect("active index"),
        original_index
    );
    assert_eq!(
        fs::read(&lock).expect("preserved foreign lock"),
        fs::read(&prepared).expect("prepared evidence")
    );
    fs::remove_file(&lock).expect("remove foreign lock");
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn retained_owned_lock_can_be_discarded_without_installing_the_index() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-discard");
    let active = dir.join(".git/index");
    let original = fs::read(&active).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::write(dir.join("base.txt"), "discarded content\n").expect("change tracked source");
    assert!(
        prepare_index_for_paths(&ctx, &active, &prepared, &["base.txt"])
            .expect("prepare distinct index")
    );
    assert_ne!(fs::read(&prepared).expect("prepared bytes"), original);

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        Err(anyhow::anyhow!("reject prepared index"))
    })
    .expect_err("guard rejection must retain the owned lock");
    assert!(discard_prepared_index_lock(&ctx, &prepared).expect("discard retained lock"));

    assert_eq!(fs::read(&active).expect("active index"), original);
    assert!(!dir.join(".git/index.lock").exists());
    assert!(!prepared_index_claim_exists(&ctx, &prepared).expect("inspect claim"));
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn discard_preserves_a_foreign_byte_identical_placeholder() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-discard-placeholder");
    let active = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active, &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        Err(anyhow::anyhow!("retain prepared lock"))
    })
    .expect_err("guard rejection must retain the owned lock");
    fs::remove_file(&lock).expect("replace owned lock");
    fs::write(&lock, b"loom index lock placeholder\n").expect("foreign placeholder");

    discard_prepared_index_lock(&ctx, &prepared)
        .expect_err("foreign placeholder must block discard");
    assert_eq!(
        fs::read(&lock).expect("preserved foreign placeholder"),
        b"loom index lock placeholder\n"
    );
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn recovery_preserves_capture_collision_behind_owned_lock() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-owned-lock-capture-collision");
    let prepared = dir.join("prepared-index");
    fs::copy(dir.join(".git/index"), &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    super::tests::seed_owned_index_lock(&ctx, &prepared, &lock);
    let capture =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-capture")
            .expect("capture path");
    fs::write(&capture, b"foreign capture collision\n").expect("foreign capture");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("capture collision behind owned lock must fail closed");

    assert_eq!(
        fs::read(&lock).expect("owned public lock"),
        fs::read(&prepared).expect("prepared bytes")
    );
    assert_eq!(
        fs::read(&capture).expect("preserved capture"),
        b"foreign capture collision\n"
    );
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn completed_recovery_preserves_a_foreign_capture_collision() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-completed-capture-collision");
    let active = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active, &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");
    super::tests::seed_owned_index_lock(&ctx, &prepared, &lock);
    let claim =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-claim")
            .expect("claim path");
    fs::remove_file(&active).expect("replace active index");
    fs::hard_link(&claim, &active).expect("publish claimed index");
    let capture =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-capture")
            .expect("capture path");
    fs::write(&capture, b"foreign completed capture\n").expect("foreign capture");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("completed publication must preserve a foreign capture");

    assert_eq!(
        fs::read(&capture).expect("preserved capture"),
        b"foreign completed capture\n"
    );
    assert!(crate::fs_util::same_file_identity_paths(&active, &claim).expect("active claim"));
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[cfg(unix)]
#[test]
fn guard_replacement_with_a_fifo_fails_without_blocking() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::FileTypeExt;

    let (ctx, dir) = super::tests::fresh_repo("prepared-index-guard-fifo");
    let active = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::write(dir.join("base.txt"), "prepared before fifo\n").expect("change tracked source");
    assert!(
        prepare_index_for_paths(&ctx, &active, &prepared, &["base.txt"])
            .expect("prepare distinct index")
    );
    let lock = dir.join(".git/index.lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        fs::remove_file(&lock)?;
        let path = CString::new(lock.as_os_str().as_bytes())?;
        let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    })
    .expect_err("FIFO replacement must fail closed");

    assert!(
        fs::symlink_metadata(&lock)
            .expect("preserved FIFO")
            .file_type()
            .is_fifo()
    );
    fs::remove_file(&lock).expect("remove FIFO");
    fs::remove_dir_all(&dir).expect("remove test repository");
}
