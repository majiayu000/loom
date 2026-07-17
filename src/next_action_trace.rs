#[cfg(debug_assertions)]
use std::fs::OpenOptions;
#[cfg(debug_assertions)]
use std::io::Write;
#[cfg(all(debug_assertions, unix))]
use std::os::fd::AsRawFd;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};

#[cfg(debug_assertions)]
use serde::Serialize;

#[cfg(debug_assertions)]
const TRACE_ENV: &str = "LOOM_TEST_NEXT_ACTION_TRACE";

#[cfg(debug_assertions)]
static TRACE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[cfg(debug_assertions)]
#[derive(Serialize)]
struct TraceRecord<'a, T> {
    emitter_id: &'static str,
    fixture_id: String,
    payload_type: &'static str,
    payload: &'a T,
}

#[cfg(debug_assertions)]
pub(crate) fn observe_next_actions<T: Serialize>(emitter_id: &'static str, payload: T) -> T {
    let Some(path) = std::env::var_os(TRACE_ENV) else {
        return payload;
    };
    let _guard = TRACE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|error| panic!("next-action trace lock poisoned: {error}"));
    let mut encoded = serde_json::to_vec(&TraceRecord {
        emitter_id,
        fixture_id: std::env::var("NEXTEST_TEST_NAME")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| std::thread::current().name().map(ToString::to_string))
            .unwrap_or_else(|| panic!("next-action trace requires an observable fixture id")),
        payload_type: std::any::type_name::<T>(),
        payload: &payload,
    })
    .unwrap_or_else(|error| panic!("next-action trace serialization failed: {error}"));
    encoded.push(b'\n');
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("next-action trace open failed for {:?}: {error}", path));
    let trace_path = std::path::Path::new(&path);
    lock_trace_file(&file, trace_path);
    file.write_all(&encoded)
        .unwrap_or_else(|error| panic!("next-action trace write failed for {:?}: {error}", path));
    file.flush()
        .unwrap_or_else(|error| panic!("next-action trace flush failed for {:?}: {error}", path));
    unlock_trace_file(&file, trace_path);
    payload
}

#[cfg(all(debug_assertions, unix))]
fn lock_trace_file(file: &std::fs::File, path: &std::path::Path) {
    // SAFETY: flock receives a live file descriptor owned by `file`.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if result != 0 {
        panic!(
            "next-action trace lock failed for {:?}: {}",
            path,
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(all(debug_assertions, unix))]
fn unlock_trace_file(file: &std::fs::File, path: &std::path::Path) {
    // SAFETY: flock receives the same live descriptor locked above.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
    if result != 0 {
        panic!(
            "next-action trace unlock failed for {:?}: {}",
            path,
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(all(debug_assertions, not(unix)))]
fn lock_trace_file(_file: &std::fs::File, _path: &std::path::Path) {}

#[cfg(all(debug_assertions, not(unix)))]
fn unlock_trace_file(_file: &std::fs::File, _path: &std::path::Path) {}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub(crate) fn observe_next_actions<T>(_emitter_id: &'static str, payload: T) -> T {
    payload
}
