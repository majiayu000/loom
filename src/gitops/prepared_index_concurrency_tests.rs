use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn placeholder_proof_path(sentinel: &Path) -> PathBuf {
    let mut path = sentinel.as_os_str().to_os_string();
    path.push(".proof");
    PathBuf::from(path)
}

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
fn discard_recovers_after_owned_placeholder_was_removed() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-discard-removed-placeholder");
    let active = dir.join(".git/index");
    let original = fs::read(&active).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::write(
        dir.join("base.txt"),
        "discarded after placeholder removal\n",
    )
    .expect("change tracked source");
    assert!(
        prepare_index_for_paths(&ctx, &active, &prepared, &["base.txt"])
            .expect("prepare distinct index")
    );

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        Err(anyhow::anyhow!("retain prepared lock"))
    })
    .expect_err("guard rejection must retain the owned lock");

    let lock = dir.join(".git/index.lock");
    let sentinel =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-sentinel")
            .expect("sentinel path");
    fs::remove_file(&lock).expect("remove retained public lock");
    fs::write(&sentinel, b"loom index lock placeholder\n").expect("create owned marker");
    let proof = placeholder_proof_path(&sentinel);
    fs::hard_link(&sentinel, &proof).expect("create exact marker proof");
    fs::hard_link(&sentinel, &lock).expect("publish owned placeholder");
    fs::remove_file(&lock).expect("simulate crash after placeholder removal");

    assert!(discard_prepared_index_lock(&ctx, &prepared).expect("resume retained-lock discard"));
    assert_eq!(fs::read(&active).expect("active index"), original);
    assert!(!sentinel.exists());
    assert!(!proof.exists());
    assert!(!prepared_index_claim_exists(&ctx, &prepared).expect("inspect claim"));
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn recovery_preserves_a_foreign_sentinel_without_a_claim() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-foreign-sentinel-no-claim");
    let active = dir.join(".git/index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active, &prepared).expect("prepared index");
    let sentinel =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-sentinel")
            .expect("sentinel path");
    fs::write(&sentinel, b"loom index lock placeholder\n").expect("foreign sentinel");

    recover_prepared_index_lock_with_guard(&ctx, &prepared, &|_| Ok(()))
        .expect_err("claimless sentinel must fail closed");

    assert_eq!(
        fs::read(&sentinel).expect("preserved foreign sentinel"),
        b"loom index lock placeholder\n"
    );
    fs::remove_dir_all(&dir).expect("remove test repository");
}

#[test]
fn discard_preserves_a_replaced_byte_identical_sentinel() {
    let (ctx, dir) = super::tests::fresh_repo("prepared-index-replaced-sentinel");
    let active = dir.join(".git/index");
    let original = fs::read(&active).expect("active index");
    let prepared = dir.join("prepared-index");
    fs::copy(&active, &prepared).expect("prepared index");
    let lock = dir.join(".git/index.lock");

    install_prepared_index_with_guard(&ctx, &prepared, &|_| {
        Err(anyhow::anyhow!("retain prepared lock"))
    })
    .expect_err("guard rejection must retain the owned lock");
    let sentinel =
        super::prepared_index_paths::prepared_index_aux_path(&ctx, &prepared, ".lock-sentinel")
            .expect("sentinel path");
    let proof = placeholder_proof_path(&sentinel);
    fs::remove_file(&lock).expect("remove retained public lock");
    fs::write(&sentinel, b"loom index lock placeholder\n").expect("owned sentinel");
    fs::hard_link(&sentinel, &proof).expect("create exact marker proof");
    fs::remove_file(&sentinel).expect("replace owned sentinel");
    fs::write(&sentinel, b"loom index lock placeholder\n").expect("foreign replacement");

    discard_prepared_index_lock(&ctx, &prepared)
        .expect_err("byte-identical sentinel replacement must fail closed");

    assert_eq!(fs::read(&active).expect("active index"), original);
    assert!(sentinel.exists());
    assert!(proof.exists());
    fs::remove_dir_all(&dir).expect("remove test repository");
}
