#[cfg(unix)]
#[test]
fn skill_tree_digest_does_not_open_fifo() {
    use std::ffi::CString;
    use std::fs::OpenOptions;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::sync::mpsc;
    use std::time::Duration;

    let root = std::env::temp_dir().join(format!(
        "loom-provenance-fifo-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("create fixture root");
    let fifo = root.join("special-node");
    let fifo_c = CString::new(fifo.as_os_str().as_bytes()).expect("fifo path");
    let created = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
    assert_eq!(
        created,
        0,
        "create fifo: {}",
        std::io::Error::last_os_error()
    );

    let worker_root = root.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = tx.send(super::skill_tree_digest(&worker_root));
    });

    let result = match rx.recv_timeout(Duration::from_secs(1)) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Unblock a regressed reader so the test can clean up and fail promptly.
            let writer = OpenOptions::new()
                .write(true)
                .custom_flags(libc::O_NONBLOCK)
                .open(&fifo)
                .expect("unblock fifo reader");
            drop(writer);
            let _ = rx.recv_timeout(Duration::from_secs(1));
            worker.join().expect("join digest worker");
            std::fs::remove_dir_all(&root).expect("remove fixture root");
            panic!("tree digest blocked while opening a fifo");
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("digest worker disconnected"),
    };

    worker.join().expect("join digest worker");
    result.expect("digest special node");
    std::fs::remove_dir_all(root).expect("remove fixture root");
}
