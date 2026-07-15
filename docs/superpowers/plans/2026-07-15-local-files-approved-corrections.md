# rs-local-files Approved Corrections Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the eight corrections approved after the second `rs-local-files` review without changing the synchronous crate boundary or the downstream `LocalTempFile::close()` contract.

**Architecture:** Keep public associated-function paths stable while moving all private filesystem machinery into `src/local/internal`. Use one shared RAII staging-file guard for atomic write and recursive file copy, validate fallible options before I/O, and cover every behavior change through public integration tests under `tests/local`.

**Tech Stack:** Rust 2024, Rust 1.94, standard-library filesystem APIs, existing `getrandom`/`libc`/`log` dependencies, Unix and Windows conditional integration tests.

## Global Constraints

- Preserve all unrelated dirty-worktree changes and do not run `git add`, `git commit`, or `git push`.
- Keep `LocalTempFile::close(&mut self) -> ()`; it releases an unbuffered handle and is not a durability API.
- Keep `LocalFileReader` and `LocalFileWriter` public and preserve every current public import path.
- New private source files live under `src/local/internal`, use the repository copyright header, contain module Rustdoc, and import their dependencies explicitly.
- Keep all tests under `tests/`; do not add inline test modules or widen production visibility for tests.
- Add each behavioral regression before its production fix and observe the expected failure whenever the host platform can execute it.
- Windows-only behavioral regressions must at least compile locally when the Windows target is installed and run in the configured `windows-2022` CI job.
- Run the correction verification sequence exactly as `./align-ci.sh`, then `./ci-check.sh`, and run `./coverage.sh json` only if CI reports coverage below threshold.

---

### Task 1: Reject dangling final child-writer symlinks

**Files:**
- Modify: `tests/local/local_temp_dir_tests.rs`
- Modify: `src/local/local_temp_dir.rs`

**Interfaces:**
- Preserves: `LocalTempDir::open_child_writer` signature.
- Produces: every existing final symlink returns `ErrorKind::InvalidInput` before opening.

- [x] **Step 1: Add the Unix regression**

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

- [x] **Step 2: Run the exact test and confirm RED**

Run:

```bash
cargo +1.94.0 test --test local_tests local::local_temp_dir_tests::test_temp_dir_open_child_writer_rejects_dangling_symlink_escape -- --exact
```

Expected: failure because the current writer follows the dangling link and creates the outside target.

- [x] **Step 3: Inspect the final component without following it**

Use `fs::symlink_metadata`; return `InvalidInput` for `file_type().is_symlink()`, retain the regular-file, non-file, missing, and metadata-error branches, and update the helper/public Rustdoc.

- [x] **Step 4: Run all temporary-directory tests**

```bash
cargo +1.94.0 test --test local_tests local::local_temp_dir_tests
```

Expected: all tests pass and the outside target remains absent.

### Task 2: Harden Windows path conversion and symlink removal

**Files:**
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/local_files.rs` before Task 7 moves the implementation

**Interfaces:**
- Produces: `wide_path(&Path) -> io::Result<Vec<u16>>`.
- Preserves: `LocalFiles::remove_any` removes the link rather than its target.

- [x] **Step 1: Add Windows NUL regressions**

Construct a target with `OsStringExt::from_wide` and assert both no-replace persistence and atomic replacement return `InvalidInput`. The atomic test first creates the NUL-prefix target and verifies its contents remain unchanged.

```rust
#[cfg(windows)]
fn path_with_interior_nul(parent: &Path, prefix: &str) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;

    let mut units: Vec<u16> = prefix.encode_utf16().collect();
    units.extend([0, u16::from(b'x')]);
    parent.join(OsString::from_wide(&units))
}
```

- [x] **Step 2: Add the Windows directory-symlink regression**

```rust
#[cfg(windows)]
#[test]
fn test_remove_any_removes_directory_symlink_without_removing_target() {
    use std::os::windows::fs::symlink_dir;

    let dir = temp_dir("remove-directory-symlink");
    let target = dir.join("target");
    let link = dir.join("link");
    fs::create_dir_all(&target).expect("target directory should be created");
    if let Err(error) = symlink_dir(&target, &link) {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    LocalFiles::remove_any(&link).expect("directory symlink should be removed");
    assert!(!link.exists());
    assert!(target.is_dir());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}
```

- [x] **Step 3: Make Windows path conversion fallible**

```rust
#[cfg(windows)]
fn wide_path(path: &Path) -> Result<Vec<u16>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path contains an interior NUL: {}", path.display()),
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}
```

Propagate `?` through replace, no-replace move, and parent-directory sync.

- [x] **Step 4: Select and harden the Windows directory-link removal API**

Under `cfg(windows)`, use `FileTypeExt::is_symlink_dir()` to select the
directory-link path. Open its final component with `CreateFileW` and
`FILE_FLAG_OPEN_REPARSE_POINT`, request `DELETE | FILE_READ_ATTRIBUTES`, verify
the same handle has directory and reparse-point attributes plus a
name-surrogate reparse tag, and mark that handle for deletion with
`SetFileInformationByHandle`. This prevents a real directory or non-link
reparse point substituted after path inspection from being deleted. Keep
`fs::remove_file` for file symlinks and Unix links.

- [x] **Step 5: Run host tests and compile Windows paths when available**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests
cargo +1.94.0 test --test local_tests local::local_temp_file_tests
rustup target list --installed
```

If `x86_64-pc-windows-gnu` is installed, additionally run:

```bash
cargo +1.94.0 test --target x86_64-pc-windows-gnu --test local_tests --no-run
```

### Task 3: Make staging cleanup panic-safe

**Files:**
- Create: `src/local/internal/mod.rs`
- Create: `src/local/internal/staged_file.rs`
- Modify: `src/local/mod.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/local_files.rs`

**Interfaces:**
- Produces: private `StagedFile` owning `Option<PathBuf>` and `Option<File>`.
- Preserves: callback panics propagate unchanged.

- [x] **Step 1: Add the panic regression**

```rust
#[test]
fn test_atomic_write_with_removes_temporary_file_when_callback_panics() {
    let dir = temp_dir("atomic-write-panic");
    let target = dir.join("state.txt");
    fs::write(&target, b"original").expect("original target should be written");

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = LocalFiles::atomic_write_with(&target, |file| {
            file.write_all(b"replacement")?;
            panic!("intentional atomic-write callback panic");
        });
    }));

    assert!(panic.is_err());
    assert_eq!(b"original", fs::read(&target).unwrap().as_slice());
    assert_no_staging_files(&dir, ".atomic-write-");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}
```

- [x] **Step 2: Run the exact test and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write_with_removes_temporary_file_when_callback_panics -- --exact
```

Expected: the callback panic is observed and the assertion finds a leaked staging file.

- [x] **Step 3: Add the private staging guard**

`StagedFile::new`, `path`, `file`, `file_mut`, `close`, and `disarm` own all staging state. `Drop` closes the handle and best-effort removes an armed path. Every type, field, method, and side effect receives complete Rustdoc.

- [x] **Step 4: Convert atomic write to the guard**

Replace the raw `(temp_path, file)` and manual abort cleanup with `StagedFile`. Clone the path only when constructing `LocalAtomicWriteError`, close before the move, and disarm only after replacement succeeds. Add `# Panics` to `atomic_write_with` stating that callback panics propagate after staging cleanup.

- [x] **Step 5: Run atomic-write tests**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write
```

Expected: all tests whose names contain `test_atomic_write` pass.

### Task 4: Stage file contents before destructive type replacement

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/local_files.rs`

**Interfaces:**
- Preserves: public copy options, errors, and statistics.
- Produces: source-open/copy failure does not remove a conflicting destination directory.

- [x] **Step 1: Add a Unix permission regression**

Create a regular source file with mode `0o000`, an existing destination directory with a marker, and use `type_conflict: Replace`. Skip only when the host can still open mode-zero files. Assert the copy fails and the marker remains.

- [x] **Step 2: Run the exact test and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_copy_dir_all_with_keeps_conflicting_directory_when_source_copy_fails -- --exact
```

Expected on a non-root Unix host: failure because the current implementation removes the destination directory before opening the source.

- [x] **Step 3: Delay removal until after staging**

Inspect conflict policy first and record `destination_directory_requires_removal`. Copy and permission preparation into `StagedFile` before calling `remove_any_path`. Immediately before removal add this comment:

```rust
// Stage the source before deleting a conflicting directory so read failures
// cannot destroy the existing destination. Removing a directory and then
// moving a file cannot be one atomic filesystem operation, so commit failure
// after this point may still leave the destination absent.
```

Use the RAII guard for every error path and disarm it after a successful commit.

- [x] **Step 4: Run all recursive-copy tests**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_copy_dir_all_with
```

Expected: all tests whose names contain `test_copy_dir_all_with` pass.

### Task 5: Complete portable Windows reserved-name validation

**Files:**
- Modify: `tests/local/local_filenames_tests.rs`
- Modify: `src/local/local_filenames.rs`

- [x] **Step 1: Add `COM¹`/`LPT¹` families and valid counterexamples to the existing reserved-name test**
- [x] **Step 2: Run the exact test and confirm RED because a superscript name is accepted**
- [x] **Step 3: Split the final Unicode scalar with `char_indices().next_back()` and accept only ASCII `1..=9` or superscripts `¹`, `²`, `³` after a case-insensitive `COM`/`LPT` prefix**
- [x] **Step 4: Add the Microsoft naming reference and target-independent portability explanation to Rustdoc and both user guides**
- [x] **Step 5: Run `cargo +1.94.0 test --test local_tests local::local_filenames_tests`**

### Task 6: Make buffering invariants explicit and enforce must-use values

**Files:**
- Modify: `src/local/file_buffering.rs`
- Modify: `src/local/file_read_options.rs`
- Modify: `src/local/file_write_options.rs`
- Modify: `src/local/local_file_reader.rs`
- Modify: `src/local/local_file_writer.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`

- [x] **Step 1: Add construction-time zero-capacity assertions and a compile-fail doctest that ignores `FileWriteOptions::default().with_parent()` under `deny(unused_must_use)`**
- [x] **Step 2: Confirm the integration test fails to compile because capacity builders return `Self`, and the compile-fail doctest fails because the ignored builder currently compiles**
- [x] **Step 3: Store `Option<NonZeroUsize>`, return `io::Result<Self>` from custom-capacity builders, and remove late reader/writer validation**
- [x] **Step 4: Add message-bearing type-level `#[must_use]` attributes; do not add redundant function-level attributes that trigger Clippy's `double_must_use`**
- [x] **Step 5: Migrate every local custom-capacity construction and run local file, reader, writer, and temp-directory tests plus doctests**

### Task 7: Split the private implementation into `local::internal`

**Files:**
- Create: `src/local/internal/path_io_error.rs`
- Create: `src/local/internal/path_operations.rs`
- Create: `src/local/internal/file_io.rs`
- Create: `src/local/internal/temp_entry.rs`
- Create: `src/local/internal/file_move.rs`
- Create: `src/local/internal/atomic_write.rs`
- Create: `src/local/internal/copy_dir.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`

- [x] **Step 1: Re-run `cargo +1.94.0 test --all-features --verbose` as the green refactor baseline**
- [x] **Step 2: Move `PathIoError` into its own file and path helpers into `path_operations.rs`**
- [x] **Step 3: Move reader/writer opening into `file_io.rs` and temporary entry creation into `temp_entry.rs`**
- [x] **Step 4: Move platform FFI, fallible path conversion, replacement/no-replace moves, and parent sync into `file_move.rs`**
- [x] **Step 5: Move the complete atomic and copy pipelines into their responsibility modules while retaining `StagedFile` as their shared internal dependency**
- [x] **Step 6: Keep `internal/mod.rs` limited to declarations and narrow `pub(crate)` re-exports; keep direct imports in every concrete file**
- [x] **Step 7: Reduce `local_files.rs` to the marker type, constants still in use, public Rustdoc, and forwarding associated methods**
- [x] **Step 8: Run all-feature tests after each move and inspect the public exports in `src/lib.rs` for unchanged paths**

### Task 8: Finish API hygiene, test layout, and documentation

**Files:**
- Modify: all affected `src/local/*.rs`, `tests/local/*.rs`, `tests/local/mod.rs`
- Modify: `README.md`, `README.zh_CN.md`, `doc/user_guide.md`, `doc/user_guide.zh_CN.md`

- [x] **Step 1: Replace `pub enum LocalFiles {}` and `pub enum LocalFilenames {}` with unconstructible marker structs containing a private `std::convert::Infallible` field; preserve associated call paths**
- [x] **Step 2: Remove the unused duplicate `LocalFiles::DEFAULT_TEMP_FILE_PREFIX` and keep `LocalFilenames::DEFAULT_RANDOM_PREFIX` canonical**
- [x] **Step 3: Add missing module Rustdoc and complete private/public item headings, including atomic callback `# Panics`, native-move same-filesystem limits, overwrite permissions, recursive-copy partial effects, and the infallible close contract**
- [x] **Step 4: Add mirrored test modules for `file_buffering`, read/write options and modes, atomic/copy error and stage types, copy stats, and local reader/writer; move the corresponding focused tests out of `local_files_tests.rs` without changing assertions**
- [x] **Step 5: Reorder inherent methods by constructor, visibility, and adjacency, then audit inline attributes using `#[inline(always)]` only for getters/setters/pure forwarding and `#[inline]` for other eligible short bodies**
- [x] **Step 6: Synchronize English and Chinese README/user-guide behavior statements and remove the inaccurate claim that temporary-file persistence flushes the unbuffered handle**
- [x] **Step 7: Run all affected test modules and doctests**

### Task 9: Verify the crate and downstream

- [x] **Step 1: Run `./align-ci.sh`, inspect all formatter/alignment changes, and preserve only in-scope changes**
- [x] **Step 2: Run `./ci-check.sh`; record its exit status and every failing stage**
- [x] **Step 3: If CI reports coverage below threshold, run exactly `./coverage.sh json`, add only meaningful in-scope tests for uncovered branches, and rerun the affected checks**
- [x] **Step 4: From `../rs-mime`, run `cargo +1.94.0 test --all-features --verbose` against the local path dependency**
- [x] **Step 5: Run `git --no-pager diff --check`, inspect status/stat/full diff, verify no public path drift, and request final code review before reporting completion**

Verification note (2026-07-15): the crate's full CI pipeline passed, including
177 integration tests, three normal doctests, five compile-fail doctests, and all
per-source coverage thresholds. `copy_dir.rs` finishes at 100% functions,
96.61% lines, and 95.2% regions. The recursive-copy ordering tests use a Linux
file lease and targeted `SIGIO` to pause the exact source-open operation; the
helper uses an absolute timeout, drains pending lease-break signals before
restoring the thread mask, and no longer polls transient staging names or
depends on large-file timing. The native `rs-mime` command currently
fails before compiling `rs-mime` because its sibling graph resolves
`qubit-datatype` 0.6 from both a path and crates.io. Applying an ephemeral
Cargo CLI patch to unify that unrelated dependency identity makes all
`rs-mime` tests and doctests pass, and the same patched graph makes
`rs-magika --all-targets --all-features` pass. No downstream file changes were
retained. Windows code and tests compile for `x86_64-pc-windows-msvc`; native
Windows runtime execution remains delegated to the configured Windows CI job.
