// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Local-filesystem fixture construction and inspection.

use std::fs;
use std::path::{
    Path,
    PathBuf,
};
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

/// Counter used to keep per-process temporary fixture names unique.
static TEST_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Creates an empty, uniquely named temporary fixture directory.
///
/// Any stale fixture at the generated path is removed before creation.
///
/// # Parameters
///
/// * `name` - Human-readable test name included in the generated path.
///
/// # Returns
///
/// Path to the created fixture directory.
///
/// # Panics
///
/// Panics when the fixture directory cannot be created.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "qubit-local-files-local-tests-{}-{name}-{id}",
        std::process::id()
    ));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[cfg(unix)]
/// Creates a short-path Unix fixture directory under `/tmp`.
///
/// Any stale fixture at the generated path is removed before creation.
///
/// # Parameters
///
/// * `name` - Human-readable test name included in the generated path.
///
/// # Returns
///
/// Path to the created fixture directory.
///
/// # Panics
///
/// Panics when the fixture directory cannot be created.
pub(crate) fn short_temp_dir(name: &str) -> PathBuf {
    let id = TEST_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path =
        PathBuf::from(format!("/tmp/qio-{}-{name}-{id}", std::process::id()));
    drop(fs::remove_dir_all(&path));
    fs::create_dir_all(&path).expect("short temp dir should be created");
    path
}

#[cfg(unix)]
/// Creates a FIFO at `path` with owner read/write permissions.
///
/// # Parameters
///
/// * `path` - Filesystem path for the FIFO.
///
/// # Panics
///
/// Panics when the path contains NUL or `mkfifo` fails.
pub(crate) fn create_fifo(path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes())
        .expect("FIFO path must not contain NUL");
    // SAFETY: `path` is a live NUL-terminated byte string and `0o600` is a
    // valid permission mode. `mkfifo` does not retain the pointer.
    let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
    assert_eq!(
        0,
        result,
        "FIFO should be created: {}",
        std::io::Error::last_os_error(),
    );
}

#[cfg(unix)]
/// Verifies that opening a FIFO returns `InvalidInput` without blocking.
///
/// If `open` blocks, this helper opens the FIFO read/write to release the
/// worker before reporting the failure, so the test process keeps no blocked
/// thread.
///
/// # Parameters
///
/// * `path` - FIFO path to open.
/// * `open` - Operation expected to reject the FIFO.
///
/// # Panics
///
/// Panics when opening blocks, the worker disconnects, or the result is not an
/// `InvalidInput` error.
pub(crate) fn assert_fifo_open_is_rejected<F>(path: PathBuf, open: F)
where
    F: FnOnce(&Path) -> std::io::Result<()> + Send + 'static,
{
    use std::sync::mpsc::{
        self,
        RecvTimeoutError,
    };
    use std::thread;
    use std::time::Duration;

    let (sender, receiver) = mpsc::channel();
    let worker_path = path.clone();
    let worker = thread::spawn(move || {
        let result = open(&worker_path);
        sender
            .send(result)
            .expect("FIFO open result should be received");
    });

    let result = match receiver.recv_timeout(Duration::from_millis(500)) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => {
            let unblocker = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .expect("read/write FIFO handle should release blocked open");
            let released_result = receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("released FIFO worker should return");
            worker.join().expect("FIFO worker should join");
            drop(unblocker);
            panic!("opening FIFO blocked before returning {released_result:?}");
        }
        Err(RecvTimeoutError::Disconnected) => {
            worker.join().expect("disconnected FIFO worker should join");
            panic!("FIFO worker disconnected before sending its result");
        }
    };
    worker.join().expect("FIFO worker should join");
    let error = result.expect_err("FIFO must be rejected");
    assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
}

#[cfg(target_os = "linux")]
/// Returns the status flags of the unique live descriptor for `path`.
///
/// The helper observes the process descriptor table through procfs so public
/// file wrappers do not need to expose their underlying handles solely for a
/// regression test.
///
/// # Parameters
///
/// * `path` - Path whose unique live descriptor should be inspected.
///
/// # Returns
///
/// Status flags parsed from `/proc/self/fdinfo`.
///
/// # Panics
///
/// Panics when the path cannot be canonicalized, procfs cannot be inspected,
/// the path does not have exactly one live descriptor, or flags are malformed.
pub(crate) fn file_status_flags(path: &Path) -> i32 {
    let canonical_path = fs::canonicalize(path)
        .expect("descriptor fixture path should be canonicalized");
    let descriptors = fs::read_dir("/proc/self/fd")
        .expect("process descriptor directory should be readable")
        .filter_map(Result::ok)
        .filter(|entry| {
            fs::read_link(entry.path())
                .is_ok_and(|target| target == canonical_path)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        1,
        descriptors.len(),
        "fixture should have exactly one live descriptor: {}",
        canonical_path.display(),
    );
    let descriptor_name = descriptors[0].file_name();
    let info = fs::read_to_string(
        Path::new("/proc/self/fdinfo").join(descriptor_name),
    )
    .expect("descriptor information should be readable");
    let flags = info
        .lines()
        .find_map(|line| line.strip_prefix("flags:\t"))
        .expect("descriptor information should contain status flags");
    i32::from_str_radix(flags.trim(), 8)
        .expect("descriptor status flags should be octal")
}

#[cfg(windows)]
/// Builds a Windows path whose final component contains an interior UTF-16 NUL.
///
/// # Parameters
///
/// * `parent` - Parent directory for the malformed component.
/// * `prefix` - Visible component prefix placed before the NUL.
///
/// # Returns
///
/// A path ending in `prefix`, an interior NUL, and the character `x`.
pub(crate) fn path_with_interior_nul(parent: &Path, prefix: &str) -> PathBuf {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let mut units: Vec<u16> = prefix.encode_utf16().collect();
    units.extend([0, u16::from(b'x')]);
    parent.join(OsString::from_wide(&units))
}

/// Counts atomic-write staging entries in a fixture directory.
///
/// # Parameters
///
/// * `dir` - Directory to scan.
///
/// # Returns
///
/// Number of direct children whose names start with `.atomic-write-`.
///
/// # Panics
///
/// Panics when the directory cannot be read.
pub(crate) fn count_atomic_temp_files(dir: &Path) -> usize {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".atomic-write-")
        })
        .count()
}
