use super::*;

#[test]
fn reentrant_lock_succeeds() {
    let dir =
        std::env::temp_dir().join(format!("loom-reentrant-{}", uuid::Uuid::new_v4().simple()));
    let ctx = AppContext::new(Some(dir.clone())).unwrap();
    let guard1 = ctx.lock_workspace().expect("first lock must succeed");
    let guard2 = ctx
        .lock_workspace()
        .expect("second reentrant lock must succeed");
    let lock_path = ctx.locks_dir.join("workspace.lock");
    assert!(
        lock_path.exists(),
        "lock file must exist while guards are held"
    );
    drop(guard1);
    drop(guard2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn inner_drop_does_not_release_file() {
    let dir =
        std::env::temp_dir().join(format!("loom-inner-drop-{}", uuid::Uuid::new_v4().simple()));
    let ctx = AppContext::new(Some(dir.clone())).unwrap();
    let guard1 = ctx.lock_workspace().unwrap();
    let guard2 = ctx.lock_workspace().unwrap();
    let lock_path = ctx.locks_dir.join("workspace.lock");
    drop(guard2);
    assert!(
        lock_path.exists(),
        "lock file must exist after inner guard drop"
    );
    drop(guard1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn outer_drop_releases_file() {
    let dir =
        std::env::temp_dir().join(format!("loom-outer-drop-{}", uuid::Uuid::new_v4().simple()));
    let ctx = AppContext::new(Some(dir.clone())).unwrap();
    let guard1 = ctx.lock_workspace().unwrap();
    let guard2 = ctx.lock_workspace().unwrap();
    let lock_path = ctx.locks_dir.join("workspace.lock");
    drop(guard2);
    drop(guard1);
    assert!(
        !lock_path.exists(),
        "lock file must not exist after all guards dropped"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cross_context_lock_is_busy() {
    let dir =
        std::env::temp_dir().join(format!("loom-cross-ctx-{}", uuid::Uuid::new_v4().simple()));
    let ctx_a = AppContext::new(Some(dir.clone())).unwrap();
    let ctx_b = AppContext::new(Some(dir.clone())).unwrap();
    let _guard = ctx_a.lock_workspace().expect("context A must acquire lock");
    let result = ctx_b.lock_workspace();
    assert!(
        result.is_err(),
        "context B must not acquire lock while A holds it"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("LOCK_BUSY"),
        "error must indicate LOCK_BUSY, got: {}",
        err_msg
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cloned_context_on_different_thread_is_busy() {
    let dir = std::env::temp_dir().join(format!(
        "loom-clone-thread-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let ctx_a = AppContext::new(Some(dir.clone())).unwrap();
    let ctx_b = ctx_a.clone();
    let _guard = ctx_a
        .lock_workspace()
        .expect("main thread must acquire lock");
    let result = std::thread::spawn(move || ctx_b.lock_workspace())
        .join()
        .expect("thread must not panic");
    assert!(
        result.is_err(),
        "cloned context on a different thread must not reenter held lock"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("LOCK_BUSY"),
        "error must indicate LOCK_BUSY, got: {}",
        err_msg
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cloned_context_different_thread_not_reaped_as_stale() {
    let dir = std::env::temp_dir().join(format!(
        "loom-stale-guard-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let ctx_a = AppContext::new(Some(dir.clone())).unwrap();
    let ctx_b = ctx_a.clone();
    let _guard = ctx_a
        .lock_workspace()
        .expect("main thread must acquire lock");

    let result = std::thread::spawn(move || ctx_b.lock_workspace())
        .join()
        .expect("thread must not panic");

    assert!(result.is_err(), "thread B must not acquire the held lock");
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("LOCK_BUSY"),
        "error must indicate LOCK_BUSY (not a stale-reap win), got: {}",
        err_msg
    );
    assert!(
        ctx_a.locks_dir.join("workspace.lock").exists(),
        "lock file must still exist after thread B was rejected"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
