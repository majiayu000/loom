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
