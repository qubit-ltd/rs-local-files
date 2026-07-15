# rs-local-files Review Follow-ups Implementation Plan

> Superseded by
> `docs/superpowers/plans/2026-07-15-local-files-approved-corrections.md`
> after the second review. In particular, do not implement this plan's
> fallible `LocalTempFile::close` or already-completed macOS CI tasks.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the reviewed filesystem correctness defects, make close and buffering states explicit, split `local_files.rs` into a public facade plus private `inner` modules, document non-transactional behavior, and enable opt-in macOS CI for `rs-local-files`.

**Architecture:** Keep every public `LocalFiles` method and its Rustdoc in `local_files.rs`, while moving private implementations into `src/local/inner`. Preserve the concrete synchronous local-filesystem boundary and the existing public reader/writer enums. Validate configuration before filesystem I/O and retain temporary-resource ownership on recoverable failures.

**Tech Stack:** Rust 2024, Rust 1.94, standard-library filesystem APIs, `NonZeroUsize`, existing `getrandom`/`libc`/`log` dependencies, GitHub Actions reusable workflows, Python/Node workflow-support tests.

## Global Constraints

- This remains a source-breaking 0.3 release; released 0.2 source compatibility is not required.
- Do not add a dependency on `qubit-fs`, an async runtime, or a new error-handling crate.
- `validate_portable_file_name` always applies the union of Windows, Linux, and macOS rules; do not conditionally compile those lexical checks.
- `LocalFileReader` and `LocalFileWriter` remain public enums.
- Rust tests stay under `tests/`; do not add inline `#[cfg(test)]` modules.
- Every behavior change starts with a regression test that fails for the expected reason.
- New private implementation files live under `src/local/inner` and import their direct dependencies explicitly.
- Preserve unrelated dirty-worktree changes. Do not add or commit `rs-local-files` or `rs-mime` changes without separate authorization.
- The user has authorized an English commit, branch merges, and pushes only for `rs-ci`.
- Stop and request direction if fetch, merge, or push reports a conflict.

---

### Task 1: Reject dangling final symlinks in child writers

**Files:**
- Modify: `tests/local/local_temp_dir_tests.rs`
- Modify: `src/local/local_temp_dir.rs`

**Interfaces:**
- Consumes: `LocalTempDir::open_child_writer`, `FileWriteOptions::default`.
- Produces: final-component symlinks always return `ErrorKind::InvalidInput` before opening or creating their targets.

- [ ] **Step 1: Add the Unix regression test**

Add this test next to the existing child symlink tests:

```rust
#[cfg(unix)]
#[test]
fn test_temp_dir_open_child_writer_rejects_dangling_symlink_escape() {
    let dir = temp_dir("temp-dir-dangling-writer-symlink");
    let temp_dir = LocalTempDir::in_dir(&dir, Some("child-"), 4)
        .expect("temp dir should be created");
    let outside = dir.join("outside.txt");
    let link = temp_dir.path().join("link.txt");
    std::os::unix::fs::symlink(&outside, &link)
        .expect("dangling symlink should be created");

    let error = temp_dir
        .open_child_writer("link.txt", FileWriteOptions::default())
        .expect_err("dangling final symlink should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(!outside.exists(), "outside target must not be created");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo +1.94.0 test --test local_tests \
  local::local_temp_dir_tests::test_temp_dir_open_child_writer_rejects_dangling_symlink_escape \
  -- --exact
```

Expected: FAIL because `open_child_writer` follows the dangling link and returns a writer instead of an error; the outside target is created.

- [ ] **Step 3: Reject final symlinks without following them**

Replace the final `fs::metadata(path)` match in `prepare_child_writer_path` with:

```rust
match fs::symlink_metadata(path) {
    Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::new(
        ErrorKind::InvalidInput,
        format!("child file target is a symbolic link: {}", path.display()),
    )),
    Ok(metadata) if metadata.is_file() => {
        ensure_existing_path_inside(root, path)
    }
    Ok(_) => Err(Error::new(
        ErrorKind::InvalidInput,
        format!("child path is not a file: {}", path.display()),
    )),
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
    Err(error) => Err(error),
}
```

Update the helper Rustdoc to state that an existing final symbolic link is rejected.

- [ ] **Step 4: Run child-path tests and verify GREEN**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_temp_dir_tests
```

Expected: every `local_temp_dir_tests` test passes, including the new regression.

- [ ] **Step 5: Inspect the focused diff without committing**

Run:

```bash
git --no-pager diff -- src/local/local_temp_dir.rs tests/local/local_temp_dir_tests.rs
```

Expected: only the regression and final-component symlink fix are present.

### Task 2: Complete portable reserved-name validation

**Files:**
- Modify: `tests/local/local_filenames_tests.rs`
- Modify: `src/local/local_filenames.rs`

**Interfaces:**
- Consumes: `LocalFilenames::validate_portable_file_name(&str) -> io::Result<()>`.
- Produces: target-independent Windows/Linux/macOS compatibility checks, including superscript Windows COM/LPT device digits.

- [ ] **Step 1: Add superscript device-name regressions**

Extend `test_validate_portable_file_name_rejects_windows_reserved_names` with:

```rust
let superscript_names = [
    "COM¹", "com².txt", "CoM³.log", "LPT¹", "lpt².txt", "LpT³.log",
];
for name in superscript_names {
    let error = LocalFilenames::validate_portable_file_name(name)
        .expect_err("superscript Windows device name should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind(), "name={name}");
}
```

Keep the existing successful assertions for `COM0.txt`, `COM10.txt`, and `LPT0.txt`.

- [ ] **Step 2: Run the reserved-name test and verify RED**

Run:

```bash
cargo +1.94.0 test --test local_tests \
  local::local_filenames_tests::test_validate_portable_file_name_rejects_windows_reserved_names \
  -- --exact
```

Expected: FAIL because the first superscript device name is accepted.

- [ ] **Step 3: Generalize the reserved suffix check**

Replace the byte-length-specific COM/LPT check with a final-character split:

```rust
let Some((suffix_index, suffix)) = base_name.char_indices().next_back() else {
    return false;
};
let prefix = &base_name[..suffix_index];
let reserved_digit = matches!(suffix, '1'..='9' | '¹' | '²' | '³');
(prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT"))
    && reserved_digit
```

Keep `CON`, `PRN`, `AUX`, `NUL`, `CONIN$`, and `CONOUT$` handling unchanged.

- [ ] **Step 4: Add platform-compatibility Rustdoc references**

Add a `# Platform compatibility` section to `validate_portable_file_name` explaining that every target applies the union of Windows, Linux, and macOS rules. Add named links to:

```text
https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file
https://www.man7.org/linux/man-pages/man7/filename.7.html
https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/intro.2.html
https://developer.apple.com/library/archive/documentation/FileManagement/Conceptual/APFS_Guide/FAQ/FAQ.html
```

State that native APIs may reject invalid names, but Windows reserved names can identify devices and portable validation intentionally runs before filesystem side effects.

- [ ] **Step 5: Run filename tests and Rustdoc**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_filenames_tests
RUSTDOCFLAGS='-D warnings' cargo +1.94.0 doc --no-deps
```

Expected: all filename tests pass and Rustdoc exits successfully without warnings.

### Task 3: Restore fallible close semantics and migrate rs-mime

**Files:**
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`
- Modify in adjacent `rs-mime`: `src/classifier/media_stream_classifier_helpers.rs`
- Modify in adjacent `rs-mime`: `src/detector/file_based_mime_detector.rs`

**Interfaces:**
- Produces: `LocalTempFile::close(&mut self) -> io::Result<()>`.
- Produces: `LocalTempFile::is_closed(&self) -> bool`.
- Changes: `LocalTempFile::keep(self) -> io::Result<PathBuf>`.
- Preserves: close failure retains the open guard; persistence failure returns `LocalPersistError<LocalTempFile>`.

- [ ] **Step 1: Replace the misleading close test with state regressions**

Use these behaviors in `local_temp_file_tests.rs`:

```rust
#[test]
fn test_temp_file_close_flushes_and_transitions_to_closed_state() {
    let dir = temp_dir("temp-file-writer-close");
    let mut file = LocalTempFile::in_dir(&dir, Some("writer-"), Some(".tmp"), 4)
        .expect("temp file should be created");
    let path = file.path().to_owned();

    file.write_all(b"payload").expect("payload should be written");
    assert!(!file.is_closed());
    file.close().expect("first close should flush and succeed");
    assert!(file.is_closed());

    let write_error = file
        .write_all(b"rejected")
        .expect_err("closed temporary file should reject writes");
    let close_error = file
        .close()
        .expect_err("second close should report closed state");

    assert_eq!(b"payload", fs::read(&path).unwrap().as_slice());
    assert_eq!(ErrorKind::NotFound, write_error.kind());
    assert_eq!(ErrorKind::NotFound, close_error.kind());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_temp_file_allows_repeated_flush_while_open() {
    let mut file = LocalTempFile::new().expect("temp file should be created");
    file.write_all(b"payload").expect("payload should be written");
    file.flush().expect("first flush should succeed");
    file.flush().expect("second flush should succeed");
    assert!(!file.is_closed());
}
```

Update existing calls to `close` and `keep` in the test file to handle their `Result` values explicitly.

- [ ] **Step 2: Run the close tests and verify RED**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_temp_file_tests
```

Expected: compile failure because `close` returns `()` and `is_closed` does not exist.

- [ ] **Step 3: Implement the close state machine**

Implement:

```rust
#[inline]
pub const fn is_closed(&self) -> bool {
    self.file.is_none()
}

pub fn close(&mut self) -> Result<()> {
    self.file
        .as_mut()
        .ok_or_else(closed_file_error)?
        .flush()?;
    drop(self.file.take());
    Ok(())
}
```

Change `cleanup` to `self.close()?`, change `keep` to return `Result<PathBuf>`, and convert a `persist_with` close error into `LocalPersistError::new(error, self)` before parent creation. In `Drop`, call `close` only when `!self.is_closed()`; log a close error, force-drop the remaining handle, and continue best-effort removal.

Write complete Rustdoc for `is_closed`, `close`, `cleanup`, `keep`, persistence, and Drop side effects. Distinguish flush from `sync_all` durability.

- [ ] **Step 4: Run temporary-file tests and verify GREEN**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_temp_file_tests
cargo +1.94.0 test --test local_tests local::local_persist_error_tests
```

Expected: all temporary-file and persist-error tests pass.

- [ ] **Step 5: Migrate rs-mime close calls**

Change both staging helpers from:

```rust
file.close();
```

to:

```rust
file.close()?;
```

No other `rs-mime` files are modified for this task.

- [ ] **Step 6: Run the affected rs-mime tests against local rs-local-files**

From the adjacent isolated `rs-mime` worktree, run:

```bash
cargo +1.94.0 test --all-features --verbose
```

Expected: all `rs-mime` tests compile and pass with the adjacent local 0.3 crate.

- [ ] **Step 7: Correct close examples and inspect diffs without committing**

Update the English and Chinese README/user-guide examples to use `close()?` and `keep()?`, and remove any claim that close guarantees durable storage. Then run:

```bash
git --no-pager diff --check
git --no-pager diff -- src/local/local_temp_file.rs tests/local/local_temp_file_tests.rs README.md README.zh_CN.md doc
```

Expected: no whitespace errors and only close-contract changes in the focused diff.

### Task 4: Make zero buffer capacity unrepresentable

**Files:**
- Modify: `src/local/file_buffering.rs`
- Modify: `src/local/file_read_options.rs`
- Modify: `src/local/file_write_options.rs`
- Modify: `src/local/local_file_reader.rs`
- Modify: `src/local/local_file_writer.rs`
- Modify: `src/local/local_files.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`

**Interfaces:**
- Produces: `FileBuffering::Buffered { capacity: Option<NonZeroUsize> }`.
- Produces: fallible `buffered_with_capacity(usize) -> io::Result<Self>` constructors/builders.
- Removes: reader/writer validation after filesystem metadata or open operations.

- [ ] **Step 1: Add construction-time zero-capacity tests**

Add assertions near the existing open-reader/open-writer capacity tests:

```rust
#[test]
fn test_buffered_options_reject_zero_capacity_during_construction() {
    let buffering = FileBuffering::buffered_with_capacity(0)
        .expect_err("zero capacity should be rejected");
    let reader = FileReadOptions::buffered_with_capacity(0)
        .expect_err("zero reader capacity should be rejected");
    let writer = FileWriteOptions::default()
        .buffered_with_capacity(0)
        .expect_err("zero writer capacity should be rejected");

    assert_eq!(ErrorKind::InvalidInput, buffering.kind());
    assert_eq!(ErrorKind::InvalidInput, reader.kind());
    assert_eq!(ErrorKind::InvalidInput, writer.kind());
}
```

- [ ] **Step 2: Run the new test and verify RED**

Run:

```bash
cargo +1.94.0 test --test local_tests \
  local::local_files_tests::test_buffered_options_reject_zero_capacity_during_construction \
  -- --exact
```

Expected: compile failure because all three current APIs return `Self`, not `Result<Self>`.

- [ ] **Step 3: Store `NonZeroUsize` and make builders fallible**

In `file_buffering.rs`, use:

```rust
use std::io::{Error, ErrorKind, Result};
use std::num::NonZeroUsize;

pub enum FileBuffering {
    Unbuffered,
    Buffered {
        capacity: Option<NonZeroUsize>,
    },
}

pub fn buffered_with_capacity(capacity: usize) -> Result<Self> {
    let capacity = NonZeroUsize::new(capacity).ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "buffer capacity must be greater than zero",
        )
    })?;
    Ok(Self::Buffered {
        capacity: Some(capacity),
    })
}
```

Propagate `Result<Self>` through the read and write options APIs. Keep `buffered()` and `unbuffered()` infallible.

- [ ] **Step 4: Remove late validation and unwrap only NonZero values**

Make `LocalFileReader::from_file` and `LocalFileWriter::from_file` infallible private constructors. Build custom buffers with `capacity.get()`. Remove `validate_buffer_capacity` and `validate_buffering`. In `open_reader_path` and `open_writer_path`, wrap the constructed reader/writer in `Ok(...)` and perform no capacity validation because invalid capacity is unrepresentable.

- [ ] **Step 5: Migrate all valid option construction**

Replace direct numeric capacity fields such as:

```rust
FileBuffering::Buffered { capacity: Some(8) }
```

with explicit successful construction:

```rust
FileBuffering::buffered_with_capacity(8)
    .expect("non-zero buffer capacity should be valid")
```

Replace reader and writer builder uses with the corresponding `expect` or `?` handling. Keep `capacity: None` uses represented through `FileBuffering::buffered()` or the options-level `buffered()` helper.

- [ ] **Step 6: Run file I/O and temp-dir tests**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests
cargo +1.94.0 test --test local_tests local::local_temp_dir_tests
```

Expected: both test modules pass; zero capacity fails during options construction and no target mutation occurs.

### Task 5: Enforce must-use builders

**Files:**
- Modify: `src/local/file_buffering.rs`
- Modify: `src/local/file_read_options.rs`
- Modify: `src/local/file_write_options.rs`

**Interfaces:**
- Produces: compile-time warnings when options or updated builder values are ignored.

- [ ] **Step 1: Add a compile-fail Rustdoc regression**

Add this example to `FileWriteOptions` Rustdoc:

```rust
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::FileWriteOptions;
///
/// FileWriteOptions::default().with_parent();
/// ```
```

- [ ] **Step 2: Run doctests and verify RED**

Run:

```bash
cargo +1.94.0 test --doc --verbose
```

Expected: FAIL because the compile-fail example compiles successfully before must-use annotations exist.

- [ ] **Step 3: Add type and builder annotations**

Add descriptive type-level attributes:

```rust
#[must_use = "buffering policies must be applied to file options"]
pub enum FileBuffering { /* existing valid variants */ }

#[must_use = "read options must be passed to LocalFiles::open_reader"]
pub struct FileReadOptions { /* existing fields */ }

#[must_use = "write options must be passed to LocalFiles::open_writer"]
pub struct FileWriteOptions { /* existing fields */ }
```

Add message-bearing `#[must_use]` to constructors and builders returning these values, including `buffered`, `unbuffered`, `new`, `with_parent`, and both capacity builders. Do not annotate boolean accessors.

- [ ] **Step 4: Run doctests and Clippy**

Run:

```bash
cargo +1.94.0 test --doc --verbose
cargo +nightly-2026-06-05 clippy --all-targets --all-features -- -D warnings
```

Expected: compile-fail doctest passes and Clippy reports no double-must-use or other warnings.

### Task 6: Split private implementations into `local::inner`

**Files:**
- Create: `src/local/inner/mod.rs`
- Create: `src/local/inner/path_operations.rs`
- Create: `src/local/inner/file_io.rs`
- Create: `src/local/inner/temp_entry.rs`
- Create: `src/local/inner/file_move.rs`
- Create: `src/local/inner/atomic_write.rs`
- Create: `src/local/inner/copy_dir.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`

**Interfaces:**
- Preserves: every existing public `LocalFiles` signature and Rustdoc location.
- Produces: private responsibility-focused modules reachable only through `local::inner`.

- [ ] **Step 1: Establish a green refactor baseline**

Run:

```bash
cargo +1.94.0 test --all-features --verbose
```

Expected: all crate tests and doctests pass before moving private code.

- [ ] **Step 2: Create the private module boundary**

Add `mod inner;` to `src/local/mod.rs`. In `inner/mod.rs`, keep child modules
private and re-export only the entry points needed by the facade and temporary
resource types:

```rust
mod atomic_write;
mod copy_dir;
mod file_io;
mod file_move;
mod path_operations;
mod temp_entry;

pub(crate) use atomic_write::{atomic_write_bytes_path, atomic_write_with_path};
pub(crate) use copy_dir::copy_dir_all_with_paths;
pub(crate) use file_io::{open_reader_path, open_writer_path};
pub(crate) use file_move::{
    move_directory_without_replacing, move_file_without_replacing, replace_file,
};
pub(crate) use path_operations::{
    clean_dir_path, dir_size_path, ensure_dir_path, ensure_parent_path,
    exists_path, list_path, metadata_path, remove_any_path,
};
pub(crate) use temp_entry::{
    create_private_dir, create_temp_dir_in_dir, create_temp_file_in_dir,
};
```

Do not place shared imports in `inner/mod.rs` and do not use `use super::*` in child modules.

- [ ] **Step 3: Move path and open helpers**

Move basic path operations, `PathIoError`, path context, `parent_dir_for`, recursive size, clean, and generic removal into `path_operations.rs`. Move `open_reader_path` and `open_writer_path` into `file_io.rs`. Expose only the functions called by the public facade or temporary-resource types as `pub(crate)`.

Update `local_files.rs` public methods to delegate through imports such as:

```rust
use super::inner::{
    clean_dir_path, dir_size_path, ensure_dir_path, ensure_parent_path,
    exists_path, list_path, metadata_path, open_reader_path, open_writer_path,
    remove_any_path,
};
```

- [ ] **Step 4: Move temporary-entry and platform move helpers**

Move unique/private temporary entry creation into `temp_entry.rs`. Move Linux/macOS/Windows FFI, replacement, no-replace moves, C/wide path conversion, and parent sync into `file_move.rs`. Keep unsafe declarations and safety comments adjacent to their use.

Update `LocalTempFile` and `LocalTempDir` to import from:

```rust
use super::inner::{
    move_directory_without_replacing, move_file_without_replacing, replace_file,
    create_private_dir, create_temp_dir_in_dir, create_temp_file_in_dir,
};
```

Import only the functions each concrete file actually uses.

- [ ] **Step 5: Move atomic write and recursive copy**

Move the complete atomic-write pipeline into `atomic_write.rs`, exposing:

```rust
pub(crate) fn atomic_write_bytes_path(
    path: &Path,
    bytes: &[u8],
) -> Result<(), LocalAtomicWriteError>;

pub(crate) fn atomic_write_with_path(
    path: &Path,
    write: &mut dyn FnMut(&mut File) -> io::Result<()>,
) -> Result<(), LocalAtomicWriteError>;
```

Move the complete recursive-copy pipeline into `copy_dir.rs`, exposing:

```rust
pub(crate) fn copy_dir_all_with_paths(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
) -> Result<LocalCopyDirStats, LocalCopyDirError>;
```

Keep all stage-aware error construction and staging cleanup in their respective modules.

- [ ] **Step 6: Keep the facade focused**

After extraction, `local_files.rs` contains only imports, the `LocalFiles` namespace and constants, public associated methods with Rustdoc, and thin callback adaptation for `atomic_write_with`. It contains no platform FFI, recursive traversal, staging loop, or private filesystem implementation.

- [ ] **Step 7: Format with the repository configuration and run full tests**

Run:

```bash
cargo +nightly-2026-06-05 fmt -- --config-path .rs-ci/rustfmt.toml
cargo +1.94.0 test --all-features --verbose
./style-check.sh
```

Expected: format succeeds, all tests pass, and style checks accept the new `inner` layout and explicit imports.

### Task 7: Document copy and persistence limits

**Files:**
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Documents: recursive copy partial effects and no rollback.
- Documents: persistence same-filesystem limitation and overwrite permission behavior.

- [ ] **Step 1: Strengthen recursive-copy Rustdoc**

Add this behavior, in prose, to `LocalFiles::copy_dir_all_with`:

```text
This operation is not a tree-level transaction. If it fails, directories and
files created or committed before the failure remain in the destination and no
rollback is attempted. Type-conflict replacement may recursively remove an
existing destination directory before a later operation fails.
```

Keep the existing structured-error and partial-statistics documentation.

- [ ] **Step 2: Strengthen persistence Rustdoc**

Document on file and directory persistence:

```text
Persistence uses a native move/rename and does not fall back to copying and
deleting. Moving across filesystems can therefore fail with EXDEV on Unix or a
platform-equivalent error.
```

For file overwrite, additionally state:

```text
Replacing an existing target keeps the temporary file's permissions; it does
not preserve the replaced target's permissions. Use atomic_write when replacing
contents while preserving existing regular-file permissions is required.
```

- [ ] **Step 3: Synchronize English and Chinese guides**

Add equivalent concise warnings to both READMEs and both user guides next to recursive copy and persistence examples. Do not imply that rollback or cross-filesystem fallback exists.

- [ ] **Step 4: Verify documentation**

Run:

```bash
cargo +1.94.0 test --doc --verbose
RUSTDOCFLAGS='-D warnings' cargo +1.94.0 doc --no-deps
python3 .rs-ci/readme-version-check.py
```

Expected: doctests, Rustdoc, and README dependency checks pass.

### Task 8: Add opt-in macOS CI in rs-ci

**Files in `rs-ci`:**
- Modify: `.github/workflows/rust-ci.yml`
- Modify: `README.md`
- Modify: `README.zh_CN.md`

**Files in `rs-local-files`:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: reusable boolean input `run_macos_tests`, default `false`.
- Produces: `macos_test` on `macos-15` when explicitly enabled.

- [ ] **Step 1: Add a static RED assertion for the missing workflow input**

Before editing the workflow, run this Python assertion from `rs-ci`:

```bash
python3 - <<'PY'
from pathlib import Path

workflow = Path('.github/workflows/rust-ci.yml').read_text()
assert 'run_macos_tests:' in workflow
assert 'macos_test:' in workflow
assert 'runs-on: macos-15' in workflow
PY
```

Expected: FAIL on the missing `run_macos_tests` assertion.

- [ ] **Step 2: Add the reusable input and macOS job**

Add this `workflow_call` input:

```yaml
run_macos_tests:
  description: Run clippy and tests on the pinned macOS runner.
  required: false
  type: boolean
  default: false
```

Add a job parallel to the Windows job:

```yaml
macos_test:
  name: macOS test
  if: ${{ inputs.run_macos_tests && github.event_name != 'schedule' }}
  runs-on: macos-15
  needs:
    - fast_checks

  steps:
    - name: Checkout source
      uses: actions/checkout@v6
      with:
        fetch-depth: 1
        submodules: recursive

    - name: Restore cargo cache
      uses: actions/cache@v5
      with:
        path: |
          ~/.cargo/registry/index
          ~/.cargo/registry/cache
          ~/.cargo/git/db
          target
        key: ${{ runner.os }}-cargo-stable-${{ hashFiles('Cargo.toml') }}-${{ hashFiles('Cargo.lock') }}
        restore-keys: |
          ${{ runner.os }}-cargo-stable-${{ hashFiles('Cargo.toml') }}-
          ${{ runner.os }}-cargo-stable-

    - name: Install Rust
      run: |
        rustup toolchain install "$RS_CI_BUILD_TOOLCHAIN" --profile minimal
        rustup component add clippy --toolchain "$RS_CI_BUILD_TOOLCHAIN"

    - name: Run clippy on macOS
      run: cargo +"$RS_CI_BUILD_TOOLCHAIN" clippy --all-targets --all-features -- -D warnings

    - name: Build and test on macOS
      run: cargo +"$RS_CI_BUILD_TOOLCHAIN" test --all-features --verbose
```

- [ ] **Step 3: Document the opt-in input**

In both `rs-ci` READMEs, add `run_macos_tests` to the reusable-workflow input documentation, stating that it defaults to false and is intended for crates with macOS-specific code paths.

- [ ] **Step 4: Enable the input only in rs-local-files**

Change the caller job to:

```yaml
jobs:
  rust-ci:
    uses: qubit-ltd/rs-ci/.github/workflows/rust-ci.yml@main
    with:
      run_macos_tests: true
    secrets: inherit
```

- [ ] **Step 5: Validate rs-ci locally**

Run from `rs-ci`:

```bash
python3 -m unittest discover -s tests -p '*_tests.py'
node --test tests/page_build_pages_tests.mjs
bash -n ./*.sh style/*.sh style/rules/*.sh
python3 - <<'PY'
from pathlib import Path
import yaml

path = Path('.github/workflows/rust-ci.yml')
yaml.load(path.read_text(), Loader=yaml.BaseLoader)
workflow = path.read_text()
assert 'run_macos_tests:' in workflow
assert 'default: false' in workflow
assert 'macos_test:' in workflow
assert "inputs.run_macos_tests && github.event_name != 'schedule'" in workflow
assert 'runs-on: macos-15' in workflow
PY
```

Expected: Python, Node, shell syntax, YAML parsing, and structural assertions all pass.

- [ ] **Step 6: Request code review before publishing rs-ci**

Use the requesting-code-review skill with the pre-change and post-change SHAs/diff. Fix all Critical and Important findings, rerun Step 5, and do not merge while those findings remain.

- [ ] **Step 7: Commit rs-ci with the authorized English message**

After checking `pwd`, `git status`, and `git diff`, run in `rs-ci`:

```bash
git add .github/workflows/rust-ci.yml README.md README.zh_CN.md
git commit -m "feat(ci): add opt-in macOS test job"
```

Expected: one rs-ci-only commit on `dev-starfish`.

- [ ] **Step 8: Fetch, merge, and push the authorized branches**

Run each command separately and stop on any conflict:

```bash
git fetch origin
git checkout dev
git merge --ff-only origin/dev
git merge --ff-only dev-starfish
git push origin dev
git checkout main
git merge --ff-only origin/main
git merge --ff-only dev
git push origin main
git checkout dev-starfish
git push origin dev-starfish
```

Expected: `dev`, `main`, and `dev-starfish` point to the new rs-ci commit locally and remotely; the active branch is `dev-starfish`.

### Task 9: Update the rs-ci submodule and run final verification

**Files:**
- Update in `rs-local-files`: `.rs-ci` gitlink
- Verify all modified source, tests, docs, and caller workflow files.

**Interfaces:**
- Consumes: published `rs-ci/main` with the opt-in macOS input.
- Produces: local `rs-local-files` worktree using the updated `.rs-ci` configuration.

- [ ] **Step 1: Update the submodule with the repository script**

From the isolated `rs-local-files` root, run:

```bash
./update-submodule.sh
```

Expected: `.rs-ci` advances to the new `rs-ci/main` commit and no other submodule changes occur.

- [ ] **Step 2: Run the complete rs-local-files verification suite**

Run:

```bash
cargo +nightly-2026-06-05 fmt -- --check --config-path .rs-ci/rustfmt.toml
./style-check.sh
cargo +nightly-2026-06-05 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-features --verbose
RUSTDOCFLAGS='-D warnings' cargo +1.94.0 doc --no-deps
```

Expected: every command exits zero, all tests and doctests pass, and no warnings are emitted.

- [ ] **Step 3: Re-run rs-mime verification**

From the adjacent isolated `rs-mime` worktree, run:

```bash
cargo +nightly-2026-06-05 fmt -- --check --config-path .rs-ci/rustfmt.toml
cargo +nightly-2026-06-05 clippy --all-targets --all-features -- -D warnings
cargo +1.94.0 test --all-features --verbose
```

Expected: formatting, Clippy, and the full rs-mime tests pass against the adjacent local rs-local-files implementation.

- [ ] **Step 4: Audit final diffs and requirements**

Run in each repository:

```bash
git status --short --branch
git --no-pager diff --check
git --no-pager diff --stat
```

Confirm the final checklist:

- dangling final symlinks are rejected;
- superscript Windows device names are rejected on every target;
- close flushes, exposes state, and rejects repeat close;
- zero capacity is rejected during options construction;
- builders are must-use;
- reader/writer public enums are unchanged;
- private code is under `local::inner`;
- copy and persistence limits are documented in both languages;
- rs-ci macOS testing is opt-in and enabled only by rs-local-files;
- rs-ci branches are pushed as authorized;
- no rs-local-files or rs-mime commit was created without authorization.

- [ ] **Step 5: Request final code review**

Use requesting-code-review for the complete rs-local-files and rs-mime diffs. Address every Critical and Important finding, then rerun the affected targeted tests and the complete verification suite before reporting completion.
