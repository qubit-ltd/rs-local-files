# rs-local-files Strong Invariants Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ordinary-file opening non-blocking against FIFOs, prevent atomic callback handle escape, expose one stable absolute path for every temporary resource, and finish the approved portability, overflow, documentation, and style corrections.

**Architecture:** Keep the existing synchronous API families, but strengthen their boundaries: `file_io` validates ordinary files around a Unix non-blocking open, `LocalAtomicWriter` itself becomes the callback capability, and temporary guards retain one absolute path. Platform fallback and checked arithmetic remain private implementation details, while external integration tests exercise every observable contract that can be reproduced reliably.

**Tech Stack:** Rust 2024, Rust 1.94.0, standard-library filesystem APIs, Unix `libc`, external integration tests, rustdoc compile-fail tests, reusable project CI scripts.

## Global Constraints

- Keep edition `2024` and minimum Rust `1.94`.
- Keep the crate synchronous; add no async runtime or filesystem abstraction.
- Ordinary reader/writer APIs return only ordinary files.
- `LocalAtomicWriter` implements `Write`, but not `Seek`, raw-handle traits, or access to its underlying `File`.
- Temporary guards retain and expose one absolute path.
- Keep all Rust tests under `tests/`; add no inline test modules and do not widen production visibility for tests.
- Use focused RED-GREEN cycles for every safely reproducible behavior change.
- Preserve unrelated worktree changes.
- Do not run `git add`, `git commit`, or `git push`.
- Do not bump the package version or edit downstream manifests.
- Use repository-standard English Rustdoc with `# Parameters`, `# Returns`, and `# Errors` where applicable.
- Keep `LocalRoot` as a separately documented follow-up; do not implement it in this plan.

---

### Task 1: Reject FIFOs and other special files without blocking

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/local/internal/file_io.rs`
- Modify: `tests/local/test_support/filesystem_fixture_tests.rs`
- Modify: `tests/local/test_support/mod.rs`
- Modify: `tests/local/local_file_reader_tests.rs`
- Modify: `tests/local/local_file_writer_tests.rs`

**Interfaces:**
- Consumes: `FileReadOptions`, `FileWriteOptions`, `LocalFileReader`, `LocalFileWriter`.
- Produces: `open_reader_path` and `open_writer_path` that return only verified ordinary files and cannot block indefinitely when an existing Unix FIFO has no peer.
- Produces test helpers: `create_fifo(&Path)` and `assert_fifo_open_is_rejected(PathBuf, F)` under `cfg(unix)`.

- [ ] **Step 1: Add safe Unix FIFO fixtures**

Add the following documented helpers to
`tests/local/test_support/filesystem_fixture_tests.rs`:

```rust
#[cfg(unix)]
/// Creates a FIFO at `path` with owner read/write permissions.
///
/// # Parameters
/// - `path`: Filesystem path for the FIFO.
///
/// # Panics
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
/// - `path`: FIFO path to open.
/// - `open`: Operation expected to reject the FIFO.
///
/// # Panics
/// Panics when opening blocks, the worker disconnects, or the result is not an
/// `InvalidInput` error.
pub(crate) fn assert_fifo_open_is_rejected<F>(path: PathBuf, open: F)
where
    F: FnOnce(&Path) -> std::io::Result<()> + Send + 'static,
{
    use std::sync::mpsc::{self, RecvTimeoutError};
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
```

Re-export both helpers from `tests/local/test_support/mod.rs` under
`#[cfg(unix)]`.

- [ ] **Step 2: Add reader and writer regressions**

Append to `tests/local/local_file_reader_tests.rs`:

```rust
#[cfg(unix)]
#[test]
fn test_open_reader_rejects_fifo_without_blocking() {
    let dir = temp_dir("open-reader-fifo");
    let fifo = dir.join("input.fifo");
    create_fifo(&fifo);

    assert_fifo_open_is_rejected(fifo, |path| {
        LocalFiles::open_reader(path, FileReadOptions::unbuffered()).map(|_| ())
    });

    fs::remove_dir_all(dir).expect("reader FIFO fixture should be removed");
}
```

Append to `tests/local/local_file_writer_tests.rs`:

```rust
#[cfg(unix)]
#[test]
fn test_open_writer_rejects_fifo_without_blocking() {
    let dir = temp_dir("open-writer-fifo");
    let fifo = dir.join("output.fifo");
    create_fifo(&fifo);

    assert_fifo_open_is_rejected(fifo, |path| {
        LocalFiles::open_writer(
            path,
            FileWriteOptions::new(FileWriteMode::OpenExistingAtStart),
        )
        .map(|_| ())
    });

    fs::remove_dir_all(dir).expect("writer FIFO fixture should be removed");
}
```

Import the two fixtures under `cfg(unix)` and import `FileWriteMode` in the
writer test.

- [ ] **Step 3: Run both regressions and confirm RED without a hung process**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_file_reader_tests::test_open_reader_rejects_fifo_without_blocking -- --exact
cargo +1.94.0 test --test local_tests local::local_file_writer_tests::test_open_writer_rejects_fifo_without_blocking -- --exact
```

Expected before the fix: each test finishes after the bounded timeout and
fails with `opening FIFO blocked`; neither command remains hung.

- [ ] **Step 4: Make `libc` available on every Unix target**

Replace:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
libc = "0.2"
```

with:

```toml
[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

- [ ] **Step 5: Add ordinary-file preflight and Unix open helpers**

In `src/local/internal/file_io.rs`, add documented private helpers with these
signatures and behavior:

```rust
/// Rejects an existing path unless it resolves to an ordinary file.
fn reject_existing_non_file(path: &Path) -> Result<()> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(path_not_regular_file_error(path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(add_path_context(error, "inspect file", path)),
    }
}

#[cfg(unix)]
/// Adds non-blocking open so a concurrent FIFO replacement cannot hang open.
fn configure_nonblocking_open(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.custom_flags(libc::O_NONBLOCK);
}

#[cfg(not(unix))]
/// Leaves ordinary platform open flags unchanged.
fn configure_nonblocking_open(_options: &mut OpenOptions) {}

#[cfg(unix)]
/// Clears the temporary non-blocking flag after handle type verification.
fn clear_nonblocking(file: &File, path: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;

    let descriptor = file.as_raw_fd();
    // SAFETY: `descriptor` belongs to the borrowed live `File`; `F_GETFL`
    // reads descriptor flags and does not retain references.
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(add_path_context(
            Error::last_os_error(),
            "read file status flags",
            path,
        ));
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: `descriptor` remains live and `F_SETFL` accepts the status flags
    // returned by `F_GETFL` with `O_NONBLOCK` removed.
    let result = unsafe {
        libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK)
    };
    if result == -1 {
        Err(add_path_context(
            Error::last_os_error(),
            "restore blocking file status",
            path,
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
/// Performs no flag restoration on platforms without Unix open flags.
fn clear_nonblocking(_file: &File, _path: &Path) -> Result<()> {
    Ok(())
}
```

Rename `opened_path_not_file_error` to `path_not_regular_file_error` and use
the message `path is not a regular file: {path}`. Keep the comment explaining
why preflight, non-blocking open, and post-open metadata are all required.

- [ ] **Step 6: Route reader and writer construction through the helpers**

Replace reader construction with the following sequence:

```rust
reject_existing_non_file(path)?;
let mut open_options = OpenOptions::new();
open_options.read(true);
configure_nonblocking_open(&mut open_options);
let file = open_options
    .open(path)
    .map_err(|error| add_path_context(error, "open file reader", path))?;
if !file.metadata()?.is_file() {
    return Err(path_not_regular_file_error(path));
}
clear_nonblocking(&file, path)?;
Ok(LocalFileReader::from_file(file, options.buffering()))
```

In writer construction, call `reject_existing_non_file(path)?` before any
parent creation, configure `O_NONBLOCK` after the existing mode flags, open the
file, reject a non-file through handle metadata, clear `O_NONBLOCK`, then wrap
the verified file:

```rust
reject_existing_non_file(path)?;
if options.creates_parent() {
    ensure_parent_path(path)?;
}
// Configure existing mode flags on `open_options` here.
configure_nonblocking_open(&mut open_options);
let file = open_options
    .open(path)
    .map_err(|error| add_path_context(error, "open file writer", path))?;
if !file.metadata()?.is_file() {
    return Err(path_not_regular_file_error(path));
}
clear_nonblocking(&file, path)?;
Ok(LocalFileWriter::from_file(file, options.buffering()))
```

Update the private and public Rustdoc errors to say that non-file resources are
rejected.

- [ ] **Step 7: Run GREEN and the complete reader/writer modules**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_file_reader_tests
cargo +1.94.0 test --test local_tests local::local_file_writer_tests
```

Expected: all reader/writer tests pass, including both FIFO regressions.

- [ ] **Step 8: Inspect the focused diff without committing**

Run:

```bash
git --no-pager diff -- Cargo.toml src/local/internal/file_io.rs tests/local/test_support tests/local/local_file_reader_tests.rs tests/local/local_file_writer_tests.rs
```

Expected: only the ordinary-file contract, Unix dependency scope, fixtures,
and related expectation wording changed.

---

### Task 2: Replace the atomic `File` callback with a guarded writer

**Files:**
- Modify: `src/local/local_atomic_writer.rs`
- Modify: `src/local/local_files.rs`
- Modify: `tests/local/local_files_tests/atomic_write_tests.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Changes: `F: FnOnce(&mut File) -> io::Result<()>` to
  `F: FnOnce(&mut LocalAtomicWriter) -> io::Result<()>`.
- Preserves: callback errors map to `WriteTemporaryFile`; panic and explicit
  errors clean the armed staging file; successful callbacks commit durably.

- [ ] **Step 1: Add a RED compile-fail contract**

Add this example to the `atomic_write_with` Rustdoc in
`src/local/local_files.rs`:

```rust
/// The callback receives a guarded writer rather than a cloneable file handle:
///
/// ```compile_fail
/// use qubit_local_files::{LocalFiles, LocalTempDir};
///
/// let dir = LocalTempDir::new()?;
/// let path = dir.path().join("state.bin");
/// let mut escaped = None;
/// LocalFiles::atomic_write_with(&path, |writer| {
///     escaped = Some(writer.try_clone()?);
///     Ok(())
/// })?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
```

- [ ] **Step 2: Confirm the doctest is RED under the old `File` signature**

Run:

```bash
cargo +1.94.0 test --doc
```

Expected: the new `compile_fail` example fails because it compiles
successfully while the callback still receives `&mut File`.

- [ ] **Step 3: Change the internal callback capability**

Replace `LocalAtomicWriter::write_with` with:

```rust
/// Invokes caller-provided staging logic and commits the destination.
///
/// The callback receives the guarded writer itself rather than `File`.
/// Exposing `File` would let the callback retain a cloned handle and mutate
/// the committed inode after rename, invalidating the atomic snapshot.
pub(crate) fn write_with<F>(
    mut self,
    write: F,
) -> Result<(), LocalAtomicWriteError>
where
    F: FnOnce(&mut Self) -> io::Result<()>,
{
    let result = write(&mut self);
    with_staging_cleanup(
        result,
        LocalAtomicWriteStage::WriteTemporaryFile,
        &self.path,
        &mut self.staged_file,
    )?;
    self.commit()
}
```

Remove the `try_clone` callback handle path and any now-unused `File` import.
Keep `write_bytes` delegating through `write_with` and `Write::write_all`.

- [ ] **Step 4: Change the public signature and documentation**

In `LocalFiles::atomic_write_with`, use:

```rust
pub fn atomic_write_with<P, F>(
    path: P,
    write: F,
) -> std::result::Result<(), LocalAtomicWriteError>
where
    P: AsRef<Path>,
    F: FnOnce(&mut LocalAtomicWriter) -> Result<()>,
{
    Self::begin_atomic_write(path)?.write_with(write)
}
```

Document that the callback receives a non-cloneable staging writer which
supports `Write` but not `Seek` or raw-handle access.

- [ ] **Step 5: Replace the obsolete handle-replacement test**

Replace `test_atomic_write_with_keeps_canonical_staging_handle` with:

```rust
#[test]
fn test_atomic_write_with_uses_guarded_atomic_writer() {
    let dir = temp_dir("atomic-guarded-callback");
    let path = dir.join("out.txt");

    LocalFiles::atomic_write_with(
        &path,
        |writer: &mut qubit_local_files::LocalAtomicWriter| {
            writer.write_all(b"committed")
        },
    )
    .expect("guarded atomic callback should commit");

    assert_eq!(
        b"committed",
        fs::read(&path)
            .expect("committed destination should be readable")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("atomic fixture should be removed");
}
```

Keep the existing callback error, cleanup failure, and panic tests; rename
their closure parameter from `file` to `writer` for the new contract.

- [ ] **Step 6: Run GREEN atomic and doctest coverage**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::atomic_write_tests
cargo +1.94.0 test --test local_tests local::local_atomic_writer_tests
cargo +1.94.0 test --doc
```

Expected: all atomic integration tests and doctests pass; the compile-fail
example now fails compilation for the intended missing `try_clone` method.

- [ ] **Step 7: Update user-facing atomic callback documentation**

In both README files and both user guides, replace wording that grants direct
temporary `File` access with wording that the callback receives a guarded
`LocalAtomicWriter`. Keep examples based on `Write::write_all`, and state that
the callback cannot clone or retain the underlying file handle.

- [ ] **Step 8: Inspect the atomic diff without committing**

Run:

```bash
git --no-pager diff -- src/local/local_atomic_writer.rs src/local/local_files.rs tests/local/local_files_tests/atomic_write_tests.rs README.md README.zh_CN.md doc/user_guide.md doc/user_guide.zh_CN.md
```

Expected: no public raw-file escape remains and no unrelated atomic semantics
change.

---

### Task 3: Collapse temporary resources to one absolute path

**Files:**
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Changes: `path`, child path helpers, `keep`, `persist`, and `persist_with`
  return stable absolute paths.
- Removes: duplicate `operation_path` state and both private
  `operation_path()` accessors.
- Preserves: cleanup, persistence recovery, Unix permissions, and current-dir
  binding of filesystem effects.

- [ ] **Step 1: Rewrite the current-dir tests as RED absolute-path contracts**

Rename the first temporary-file test to
`test_temp_file_exposes_absolute_location_after_cwd_change`. Its central
assertions become:

```rust
let file = LocalTempFile::in_dir("temp", Some("cwd-"), None, 4)
    .expect("relative temporary file should be created");
assert!(file.path().is_absolute());
assert!(file.path().starts_with(creation_dir.join("temp")));
let generated_path = file.path().to_owned();
std::env::set_current_dir(&later_dir)
    .expect("current directory should change");
assert!(file.exists().expect("existence should be checked"));
assert!(generated_path.exists());
drop(file);
assert!(!generated_path.exists());
```

Rename the matching directory test to
`test_temp_dir_exposes_absolute_location_after_cwd_change` and assert:

```rust
assert!(temp_dir.path().is_absolute());
assert!(temp_dir.path().starts_with(creation_dir.join("temp")));
let generated_path = temp_dir.path().to_owned();
std::env::set_current_dir(&later_dir)
    .expect("current directory should change");
let child = temp_dir
    .ensure_child_dir("nested")
    .expect("absolute child directory should be created");
assert_eq!(generated_path.join("nested"), child);
```

- [ ] **Step 2: Add RED `keep` and `persist` result tests**

Add one file and one directory test for `keep` from a relative creation parent:

```rust
assert!(kept_path.is_absolute());
assert!(kept_path.starts_with(creation_dir.join("temp")));
assert!(kept_path.exists());
```

Add one file and one directory persistence test that changes into a fixture
directory, passes `"persisted/..."` as the target, and asserts:

```rust
assert!(persisted_path.is_absolute());
assert_eq!(creation_dir.join("persisted/final-entry"), persisted_path);
assert!(persisted_path.exists());
```

Use `CURRENT_DIR_LOCK` and `CurrentDirGuard` in all four tests, restore cwd
before removing fixtures, and use distinct final names for files and
directories.

- [ ] **Step 3: Run the six absolute-path tests and confirm RED**

Run the exact names listed by:

```bash
cargo +1.94.0 test --test local_tests -- --list | rg 'absolute_location|keep_returns_absolute|persist_returns_absolute'
```

Then run each listed name with `--exact`. Expected before implementation:
`path` and `keep` assertions fail because relative spelling is returned, and
`persist` assertions fail because the caller's relative target is returned.

- [ ] **Step 4: Reduce `LocalTempFile` to one path field**

Change the struct and construction to:

```rust
pub struct LocalTempFile {
    /// Absolute generated path while cleanup remains armed.
    path: Option<PathBuf>,
    /// Original unbuffered file handle until explicitly closed.
    file: Option<File>,
}

let operation_dir = absolute_path(dir.as_ref())?;
let (path, file) = create_temp_file_in_dir(
    &operation_dir,
    prefix,
    suffix,
    max_tries,
)?;
Ok(Self {
    path: Some(path),
    file: Some(file),
})
```

Route `exists`, `metadata`, cleanup, and persistence source operations through
`self.path()`. Remove `operation_path`, `operation_path()`, and every paired
`take`. On successful persistence return the absolute target:

```rust
let target = match absolute_path(target.as_ref()) {
    Ok(path) => path,
    Err(error) => return Err(LocalPersistError::new(error, self)),
};
if let Err(error) = LocalFiles::ensure_parent(&target) {
    return Err(LocalPersistError::new(error, self));
}
let source = self
    .path
    .as_ref()
    .expect("temporary file path has already been released");
// Perform the selected move from `source` to `target`.
let _ = self.path.take();
Ok(target)
```

Preserve `LocalPersistError::new(error, self)` at each fallible boundary rather
than using `?` where it would lose the guard.

Drop becomes:

```rust
fn drop(&mut self) {
    self.close();
    if let Some(path) = self.path.take()
        && let Err(error) = fs::remove_file(&path)
    {
        warn!("failed to remove temporary file {}: {}", path.display(), error);
    }
}
```

- [ ] **Step 5: Reduce `LocalTempDir` to one path field**

Change the struct and construction to:

```rust
pub struct LocalTempDir {
    /// Absolute generated path while cleanup remains armed.
    path: Option<PathBuf>,
}

let operation_dir = absolute_path(dir.as_ref())?;
let path = create_temp_dir_in_dir(&operation_dir, prefix, max_tries)?;
Ok(Self { path: Some(path) })
```

Use `self.path()` for metadata, listing, child resolution, containment helpers,
cleanup, persistence source, and drop. `keep` takes and returns the sole path.
Successful `persist` returns `operation_target` rather than the original
relative target. Remove `operation_path`, `operation_path()`, and paired
disarming.

- [ ] **Step 6: Run GREEN temporary-resource modules**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_temp_file_tests
cargo +1.94.0 test --test local_tests local::local_temp_dir_tests
```

Expected: all existing lifecycle, permission, child, cleanup, and persistence
tests plus the new absolute-path tests pass.

- [ ] **Step 7: Update the absolute-path documentation contract**

In Rustdoc, README files, and user guides:

- remove every statement that caller-visible generated paths preserve relative
  spelling;
- state that relative creation directories and persistence targets are bound
  when the operation begins and all returned paths are absolute;
- state that `path`, child helpers, `keep`, `persist`, and `persist_with` remain
  directly usable after later cwd changes.

Keep the existing Windows path-length and verbatim-path caveats.

- [ ] **Step 8: Search for stale dual-path state and wording**

Run:

```bash
rg -n 'operation_path|relative spelling|相对拼写' src tests README.md README.zh_CN.md doc
```

Expected: no temporary-file or temporary-directory dual-path field/accessor and
no caller-facing relative-spelling promise remain. `LocalAtomicWriter` may
still contain `operation_path` because it intentionally keeps requested error
spelling separate from its bound destination.

---

### Task 4: Add the non-Unix/non-Windows no-replace fallback

**Files:**
- Modify: `src/local/internal/file_move.rs`
- Modify: `tests/local/local_temp_file_tests.rs`

**Interfaces:**
- Produces: a definition of `move_file_without_replacing` on every target.
- Preserves: native Linux/macOS/Windows implementations and the other-Unix
  hard-link implementation.

- [ ] **Step 1: Add the cfg-specific integration contract**

Append to `tests/local/local_temp_file_tests.rs`:

```rust
#[cfg(not(any(unix, windows)))]
#[test]
fn test_temp_file_persist_reports_unsupported_no_replace_move() {
    let dir = temp_dir("unsupported-file-persist");
    let file = LocalTempFile::in_dir(&dir, Some("source-"), None, 4)
        .expect("temporary file should be created");
    let target = dir.join("target.txt");

    let error = file
        .persist(&target)
        .expect_err("no-replace file move should be unsupported");

    assert_eq!(ErrorKind::Unsupported, error.kind());
    error
        .resource
        .cleanup()
        .expect("failed persistence resource should be cleaned up");
    fs::remove_dir_all(dir).expect("unsupported fixture should be removed");
}
```

This test is intentionally not executed on a supported host; it exists to
compile and run on the affected cfg.

- [ ] **Step 2: Add the fallback implementation**

Add beside the existing directory fallback in `file_move.rs`:

```rust
/// Rejects no-replace file persistence on unsupported targets.
///
/// # Parameters
/// - `source`: Existing source file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Always returns [`ErrorKind::Unsupported`] because this target has no native
/// or hard-link no-replace file move implementation.
#[cfg(not(any(unix, windows)))]
pub(crate) fn move_file_without_replacing(
    source: &Path,
    destination: &Path,
) -> Result<()> {
    Err(Error::new(
        ErrorKind::Unsupported,
        format!(
            "moving file '{}' to '{}' without replacement is unsupported",
            source.display(),
            destination.display(),
        ),
    ))
}
```

- [ ] **Step 3: Verify host cfg and attempt the target compile**

Run:

```bash
rustc +1.94.0 --print cfg --target wasm32-wasip1 | rg 'target_(family|os)'
rustup target list --installed | rg '^wasm32-wasip1$'
```

If installed, run:

```bash
cargo +1.94.0 check --tests --target wasm32-wasip1
```

Expected: static cfg output identifies WASI as non-Unix/non-Windows, and the
crate no longer has a missing `move_file_without_replacing` export. If the
target is absent, record that target compilation remains unchecked; do not
install toolchains or targets without separate authorization.

---

### Task 5: Make directory-size overflow deterministic

**Files:**
- Modify: `src/local/internal/path_operations.rs`
- Inspect: `tests/local/local_files_tests/path_operation_tests.rs`

**Interfaces:**
- Preserves: `LocalFiles::dir_size<P>(P) -> io::Result<u64>`.
- Changes: aggregate overflow returns contextual `InvalidData` instead of
  debug panic or release wrapping.

- [ ] **Step 1: Run the existing directory-size baseline**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::path_operation_tests::test_dir_size
```

Expected: normal recursive, symlink, missing-path, and permission cases pass.
No portable RED overflow fixture is added because reliably creating more than
`u64::MAX` bytes of aggregate file length would require an artificial
filesystem seam or production visibility solely for testing.

- [ ] **Step 2: Replace unchecked additions with one checked contribution**

Inside `dir_size_recursive`, compute the contribution once and use
`checked_add`:

```rust
let entry_path = entry.path();
let metadata = fs::symlink_metadata(&entry_path)?;
let file_type = metadata.file_type();
if file_type.is_symlink() {
    continue;
}
let contribution = if metadata.is_dir() {
    dir_size_recursive(&entry_path)?
} else if metadata.is_file() {
    metadata.len()
} else {
    0
};
total = total.checked_add(contribution).ok_or_else(|| {
    Error::new(
        ErrorKind::InvalidData,
        format!("directory size exceeds u64 at {}", entry_path.display()),
    )
})?;
```

Use the existing `Error`, `ErrorKind`, and `Result` imports, and update
`dir_size` Rustdoc to include aggregate overflow among error cases.

- [ ] **Step 3: Run the path-operation module after the change**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::path_operation_tests
```

Expected: all path-operation tests pass with unchanged ordinary totals.

---

### Task 6: Apply the approved documentation and Rust style cleanup

**Files:**
- Modify: `src/local/local_filenames.rs`
- Modify: `src/local/file_read_options.rs`
- Modify: `src/local/local_atomic_writer.rs`
- Modify: `src/local/local_files.rs`
- Modify: `tests/local/local_filenames_tests.rs`
- Modify: any tests already touched by Tasks 1–5 that contain incidental
  `unwrap` calls
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Preserves: portable filename behavior and all non-atomic public signatures.
- Produces: documentation matching Unicode control rejection and consistent
  repository inline/method-order conventions.

- [ ] **Step 1: Add the Unicode control regression**

Add `"next\u{0085}line.txt"` to the invalid-name table in
`test_validate_portable_file_name_rejects_path_and_reserved_characters`.

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_filenames_tests::test_validate_portable_file_name_rejects_path_and_reserved_characters -- --exact
```

Expected: the test passes immediately because behavior is already correct; it
locks the documentation correction rather than a runtime behavior change.

- [ ] **Step 2: Correct filename wording and inline annotations**

Change `ASCII control characters` to `control characters` in
`LocalFilenames::validate_portable_file_name` Rustdoc. Make these exact inline
changes:

```rust
// Pure construction/delegation.
#[inline(always)]
pub fn buffered_with_capacity(...)

#[inline(always)]
fn LocalAtomicWriter::write(...)

#[inline(always)]
fn LocalAtomicWriter::flush(...)

// Contains a match branch.
#[inline]
pub fn LocalFilenames::file_name_from_path(...)
```

- [ ] **Step 3: Group atomic methods by responsibility**

Move the complete `begin_atomic_write` method block from the beginning of the
`LocalFiles` implementation to immediately before `atomic_write`. Do not alter
its body, generics, error type, or documentation except for callback links
already required by Task 2.

- [ ] **Step 4: Clean only touched-test diagnostics**

In test functions modified by this plan, replace incidental `.unwrap()` with
`.expect("specific operation should succeed")`. Do not perform a global
replacement and do not edit untouched test functions solely for style.

- [ ] **Step 5: Finish bilingual documentation synchronization**

Review all four user-facing documents and ensure they consistently state:

- reader/writer helpers reject special files;
- `atomic_write_with` receives a guarded writer, not a `File`;
- temporary paths and persistence results are absolute;
- returned absolute paths stay usable after cwd changes;
- current atomic durability, cleanup, and Windows native-path caveats remain.

Run:

```bash
rg -n 'direct access.*file handle|直接使用临时文件句柄|relative spelling|相对拼写|ASCII control' README.md README.zh_CN.md doc src
```

Expected: no stale contract wording remains.

- [ ] **Step 6: Format and run focused style-sensitive tests**

Run:

```bash
cargo +1.94.0 fmt --all -- --check
cargo +1.94.0 test --test local_tests local::local_filenames_tests
cargo +1.94.0 test --doc
```

If rustfmt reports changes, run `cargo +1.94.0 fmt --all`, inspect the diff,
then rerun the check. Expected: formatting, filename tests, and doctests pass.

---

### Task 7: Full verification and downstream check

**Files:**
- Inspect: all modified source, tests, docs, and design/plan files
- Inspect: direct downstream `../rs-mime`

**Interfaces:**
- Verifies: crate behavior, docs, lint/build gates, coverage policy, and the
  in-tree direct consumer.

- [ ] **Step 1: Run complete local tests and all-target compilation**

Run:

```bash
cargo +1.94.0 test --all-targets
cargo +1.94.0 test --doc
cargo +1.94.0 check --all-targets
```

Expected: all commands exit zero with no failed tests or compilation errors.

- [ ] **Step 2: Run project alignment and CI checks**

Run in this order:

```bash
./align-ci.sh
./ci-check.sh
```

Expected: both scripts exit zero. If `align-ci.sh` modifies generated CI files,
inspect and retain only changes produced by the repository's alignment flow.

- [ ] **Step 3: Apply the repository coverage policy**

If `ci-check.sh` reports coverage below its configured threshold, run:

```bash
./coverage.sh json
```

Expected: coverage output identifies no new uncovered behavior requiring a
safe external regression. If the threshold already passes, record that the
conditional coverage script was not required.

- [ ] **Step 4: Verify the direct downstream path dependency**

Run:

```bash
cargo +1.94.0 test --manifest-path ../rs-mime/Cargo.toml
```

Expected: `rs-mime` compiles and its tests pass without source edits. Its
production calls use ordinary regular files and treat temporary paths as
opaque `Path` values.

- [ ] **Step 5: Review final diff and repository state**

Run:

```bash
git status --short
git --no-pager diff --check
git --no-pager diff --stat
git --no-pager diff
```

Expected: only approved `rs-local-files` source/tests/docs plus the two design
documents and this implementation plan are changed; no downstream repository,
manifest version, lockfile, or unrelated file is modified.

- [ ] **Step 6: Perform the final requirement audit**

Confirm explicitly before reporting completion:

- FIFO reader and writer tests demonstrated RED without hanging and now pass;
- atomic callbacks cannot obtain or clone `File` handles;
- every temporary-resource path/result is absolute and backed by one field;
- non-Unix/non-Windows builds have an `Unsupported` fallback;
- directory-size accumulation uses `checked_add`;
- approved docs/style corrections are present;
- `LocalRoot` remains design-only;
- no commit or push occurred;
- any unavailable platform verification is reported rather than inferred.
