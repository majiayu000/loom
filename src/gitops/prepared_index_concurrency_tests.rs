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
