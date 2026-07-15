# rs-local-files 0.3 Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved source-breaking 0.3 filesystem hardening with regression-first tests and migrate the only production downstream crate.

**Architecture:** Keep the flat public facade while adding focused policy and error types. Temporary files own their original handle, recursive copy stages each file in the destination directory before committing, persistence errors return ownership of their guard, and complex operations expose stage-aware errors while simple helpers keep `io::Result`.

**Tech Stack:** Rust 2024, Rust 1.94, standard-library filesystem APIs, existing `libc`/`getrandom`/`log` dependencies, integration tests under `tests/local`.

## Global Constraints

- Public API compatibility with 0.2 is intentionally not preserved.
- Do not add a dependency on `qubit-fs` or an async runtime.
- Every production behavior change must be preceded by a regression test that fails for the expected reason.
- Rust tests remain under `tests/`; do not add inline `#[cfg(test)]` modules.
- Do not commit, add, or push unless the user explicitly requests it.
- Preserve unrelated worktree changes.

---

### Task 1: Private temporary modes and mutation-free write validation

**Files:**
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_temp_dir.rs`

**Interfaces:**
- Consumes: existing `LocalTempFile`, `LocalTempDir`, and `LocalFiles::open_writer` APIs.
- Produces: Unix temporary mode `0o600`/`0o700`; invalid buffering errors before filesystem mutation.

- [ ] **Step 1: Add failing Unix permission tests**

```rust
#[cfg(unix)]
#[test]
fn test_temp_file_uses_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let file = LocalTempFile::new().expect("temporary file should be created");
    let mode = file
        .metadata()
        .expect("temporary file metadata should be readable")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(0o600, mode);
}
```

Add the corresponding directory assertion for `0o700` and a child-directory
assertion under `LocalTempDir`.

- [ ] **Step 2: Extend the zero-capacity writer regression**

```rust
#[test]
fn test_open_writer_rejects_zero_buffer_capacity_without_mutating_target() {
    let dir = temp_dir("open-writer-zero-capacity");
    let path = dir.join("nested").join("data.txt");
    fs::create_dir_all(path.parent().expect("path should have a parent"))
        .expect("parent should be created");
    fs::write(&path, b"original").expect("fixture should be written");

    let error = LocalFiles::open_writer(
        &path,
        FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
            .buffered_with_capacity(0),
    )
    .expect_err("zero-capacity writer buffer should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(b"original", fs::read(&path).expect("target should remain readable"));
}
```

Add a second assertion that `create_parent=true` does not create missing
parents when validation fails.

- [ ] **Step 3: Run the new tests and verify RED**

Run:

```bash
cargo test --test tests local::local_temp_file_tests::test_temp_file_uses_private_permissions
cargo test --test tests local::local_temp_dir_tests::test_temp_dir_uses_private_permissions
cargo test --test tests local::local_files_tests::test_open_writer_rejects_zero_buffer_capacity_without_mutating_target
```

Expected: permission assertions observe umask-derived modes and the writer test
observes truncated contents before the fix.

- [ ] **Step 4: Implement private creation and pre-open validation**

On Unix use `OpenOptionsExt::mode(0o600)` and
`DirBuilderExt::mode(0o700)`. Extract a buffering validator and call it at the
start of `open_writer_path`, before `ensure_parent_path` or `OpenOptions::open`.

- [ ] **Step 5: Run the three targeted test groups and verify GREEN**

Run the commands from Step 3 and confirm all tests pass.

### Task 2: Explicit copy conflict policies and correct permission behavior

**Files:**
- Create: `src/local/local_copy_conflict_policy.rs`
- Create: `src/local/local_copy_type_conflict_policy.rs`
- Create: `tests/local/local_copy_conflict_policy_tests.rs`
- Create: `tests/local/local_copy_type_conflict_policy_tests.rs`
- Modify: `src/local/local_copy_dir_options.rs`
- Modify: `src/local/local_copy_dir_stats.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/local/mod.rs`
- Modify: `tests/local/local_copy_dir_options_tests.rs`
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/local_files.rs`

**Interfaces:**
- Produces: `LocalCopyConflictPolicy::{Fail, Overwrite, Skip}`.
- Produces: `LocalCopyTypeConflictPolicy::{Fail, Replace}`.
- Produces: `LocalCopyDirStats::skipped`.
- Removes: `LocalCopyDirOptions::overwrite`.

- [ ] **Step 1: Add policy and permission regression tests**

Add tests that demonstrate:

```rust
assert_eq!(0o600, destination_mode & 0o777);
assert_eq!(b"old", fs::read(&skipped_path).expect("skipped file should remain"));
assert_eq!(1, skipped_stats.skipped);
assert!(destination_directory.join("unrelated.txt").exists());
```

The last assertion uses `conflict: Overwrite` with `type_conflict: Fail` and a
source-file/destination-directory mismatch. Add a separate `Replace` test that
explicitly permits recursive replacement.

- [ ] **Step 2: Run copy tests and verify RED or compile failure**

Run:

```bash
cargo test --test local_tests local::local_files_tests::test_copy_dir_all_with_does_not_preserve_permissions_by_default
cargo test --test local_tests local::local_files_tests::test_copy_dir_all_with_skips_existing_files
cargo test --test local_tests local::local_files_tests::test_copy_dir_all_with_rejects_type_conflict_without_removing_directory
```

Expected: the permission test fails and the new policy API does not compile.

- [ ] **Step 3: Add the new policy types and update options/defaults**

```rust
pub enum LocalCopyConflictPolicy {
    Fail,
    Overwrite,
    Skip,
}

pub enum LocalCopyTypeConflictPolicy {
    Fail,
    Replace,
}
```

The default is `Fail` for both policies. Update all existing call sites and
tests from `overwrite: bool` to `conflict`.

- [ ] **Step 4: Replace direct `fs::copy` commits with staging commits**

Copy each source file to a private same-directory temporary file. Apply source
permissions only when requested, then commit with
`move_file_without_replacing` for `Fail`/`Skip` and `replace_file` for
`Overwrite`. `Skip` converts an `AlreadyExists` commit error into a skipped
counter increment. Recursive directory deletion is reachable only through
`LocalCopyTypeConflictPolicy::Replace`.

- [ ] **Step 5: Run all recursive-copy tests and verify GREEN**

Run:

```bash
cargo test --test tests local::local_copy_dir_options_tests
cargo test --test tests local::local_copy_conflict_policy_tests
cargo test --test tests local::local_copy_type_conflict_policy_tests
cargo test --test local_tests local::local_files_tests::test_copy_dir_all_with
```

### Task 3: Structured recursive-copy errors

**Files:**
- Create: `src/local/local_copy_dir_error.rs`
- Create: `src/local/local_copy_dir_stage.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/local/local_files.rs`
- Modify: `tests/local/local_files_tests.rs`

**Interfaces:**
- Produces: `LocalCopyDirError` with stage, source path, destination path,
  partial stats, and native source error.
- Produces: `LocalCopyDirStage`.
- Changes: `LocalFiles::copy_dir_all_with` error type from `io::Error` to
  `LocalCopyDirError`.

- [ ] **Step 1: Add a failing structured-error assertion**

```rust
let error = LocalFiles::copy_dir_all_with(&src, &dst, options)
    .expect_err("unsupported source entry should fail");
assert_eq!(LocalCopyDirStage::InspectSource, error.stage);
assert_eq!(socket_path, error.source_path);
assert_eq!(dst.join("socket"), error.destination_path);
assert_eq!(ErrorKind::Unsupported, error.source.kind());
assert_eq!(LocalCopyDirStats::default(), error.stats);
```

- [ ] **Step 2: Run the focused test and verify compile failure**

Run the exact test with `cargo test --test tests <test-name>` and confirm the
new error fields are absent.

- [ ] **Step 3: Implement stage-aware error propagation**

Define `Display` and `std::error::Error`, preserving the original `io::Error`
from `source()`. Thread a mutable statistics snapshot through all recursive
error constructors.

- [ ] **Step 4: Run all copy tests and verify GREEN**

Run `cargo test --test local_tests local::local_files_tests::test_copy_dir_all_with`.

### Task 4: Direct-write temporary files and recoverable persistence

**Files:**
- Create: `src/local/local_persist_error.rs`
- Create: `tests/local/local_persist_error_tests.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Modify: `tests/local/mod.rs`
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`

**Interfaces:**
- Removes: `LocalTempFile::writer(FileWriteOptions)`.
- Produces: `Write` and `Seek` implementations for `LocalTempFile`.
- Produces: `LocalTempFile::as_file`, `as_file_mut`, and failure-safe `close`.
- Produces: generic `LocalPersistError<T>` with `error`, `resource`, and
  `into_parts` access.

- [ ] **Step 1: Replace writer tests with desired direct-write tests**

```rust
#[test]
fn test_temp_file_writes_through_owned_handle() {
    let mut file = LocalTempFile::new().expect("temporary file should be created");
    file.write_all(b"payload").expect("payload should be written");
    file.close().expect("temporary file should close");
    assert_eq!(b"payload", fs::read(file.path()).expect("payload should be readable"));
}
```

Add a test proving writes after close return `NotFound` and a test proving a
failed no-overwrite persist returns the original guard with its contents.

- [ ] **Step 2: Run tests and verify compile failure**

The direct `write_all` call must fail to compile before the implementation.

- [ ] **Step 3: Simplify `LocalTempFile` to `Option<File>`**

Implement `Write`/`Seek` by delegating through a helper that returns
`NotFound` after close. `close` calls `flush` on the borrowed handle and takes
the `Option<File>` only after success. Remove the old writer-state enum and all
`FileWriteOptions` handling.

- [ ] **Step 4: Implement `LocalPersistError<T>` and migrate persist methods**

```rust
pub struct LocalPersistError<T> {
    pub error: io::Error,
    pub resource: T,
}
```

Implement `Display`, `Error`, and `into_parts`. Every close, parent creation,
or move error returns the still-owned resource.

- [ ] **Step 5: Run temporary resource tests and verify GREEN**

Run:

```bash
cargo test --test tests local::local_temp_file_tests
cargo test --test tests local::local_temp_dir_tests
cargo test --test tests local::local_persist_error_tests
```

### Task 5: Race-resistant directory persistence and explicit child-helper contract

**Files:**
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_temp_dir.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Changes: `LocalTempDir::persist` uses native no-replace movement.
- Contract: child helpers are lexical/best-effort validation, not a concurrent
  mutation security boundary.

- [ ] **Step 1: Add recoverable existing-target persist regression**

Assert that an existing target returns `AlreadyExists`, the returned error owns
the original `LocalTempDir`, and both the temporary directory and target retain
their original contents.

- [ ] **Step 2: Run the test and verify RED**

Expected: the current `io::Error` does not return the guard.

- [ ] **Step 3: Reuse native no-replace movement for directories**

Use `move_path_without_replacing` on Linux, macOS, and Windows. Return
`Unsupported` on platforms where an atomic no-replace directory move is not
implemented; do not fall back to check-then-rename.

- [ ] **Step 4: Update child-helper documentation**

State in API docs, README, and user guides that helpers reject lexical
traversal and observed symlinks but cannot defend against an actor concurrently
mutating the directory tree.

- [ ] **Step 5: Run directory tests and doctests**

Run `cargo test --test tests local::local_temp_dir_tests` and
`cargo test --doc`.

### Task 6: Structured atomic-write errors and retained simple-error sources

**Files:**
- Create: `src/local/local_atomic_write_error.rs`
- Create: `src/local/local_atomic_write_stage.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/local/local_files.rs`
- Modify: `tests/local/local_files_tests.rs`

**Interfaces:**
- Produces: `LocalAtomicWriteError` and `LocalAtomicWriteStage`.
- Changes: `LocalFiles::atomic_write` and `atomic_write_with` return the new
  error type; `atomic_write_with` accepts `FnOnce`.
- Preserves: original errors in simple path-context error source chains.

- [ ] **Step 1: Add stage and source-chain regressions**

```rust
let error = LocalFiles::atomic_write_with(&path, |_file| {
    Err(Error::new(ErrorKind::Other, "writer failed"))
})
.expect_err("writer failure should be returned");
assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage);
assert!(!error.committed);
assert_eq!(ErrorKind::Other, error.source.kind());
```

Add a simple `open_reader` missing-path assertion that
`std::error::Error::source(&error).is_some()`.

- [ ] **Step 2: Run regressions and verify RED**

Confirm the atomic test fails to compile on the old error type and the simple
error source assertion fails.

- [ ] **Step 3: Implement atomic stage mapping**

Map parent preparation, destination inspection, staging creation, callback,
permissions, flush, file sync, replacement, and parent sync separately. Set
`committed=true` only after replacement succeeds. Preserve the staging path in
errors when available.

- [ ] **Step 4: Preserve path-context sources**

Wrap the original `io::Error` in a private path-context error implementing
`Display` and `Error`; do not flatten it into a formatted string.

- [ ] **Step 5: Run atomic and reader tests and verify GREEN**

Run the focused tests, followed by `cargo test --test tests`.

### Task 7: Version, documentation, formatting, and downstream migration

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`
- Modify: `/home/starfish/working/qubit/rust-common/rs-mime/Cargo.toml`
- Modify: `/home/starfish/working/qubit/rust-common/rs-mime/Cargo.lock`
- Modify: `/home/starfish/working/qubit/rust-common/rs-mime/src/detector/file_based_mime_detector.rs`
- Modify: `/home/starfish/working/qubit/rust-common/rs-mime/src/classifier/media_stream_classifier_helpers.rs`

**Interfaces:**
- Changes package version to `0.3.0` and downstream requirement to `0.3`.
- Migrates `rs-mime` to direct `LocalTempFile` writes.

- [ ] **Step 1: Update `rs-mime` tests or compile target before implementation**

Run `rs-mime` against a local crates.io patch after the `LocalTempFile` API
change and confirm the old `close` plus `fs::write`/`File::create` staging code
is the remaining migration site.

- [ ] **Step 2: Migrate staging call sites**

Use `Write::write_all` for byte slices and `io::copy(reader, &mut file)` for
streams, then call `close()` before the native path callback.

- [ ] **Step 3: Update public documentation and versions**

Document private default modes, direct temporary writes, copy conflict
policies, recoverable persistence errors, partial copy statistics, and atomic
error stages. Update all install snippets from `0.2` to `0.3`.

- [ ] **Step 4: Apply project formatting**

Run:

```bash
cargo +nightly-2026-06-05 fmt -- --config-path ../rs-ci/rustfmt.toml
```

Run the corresponding formatter in `rs-mime` with its configured rustfmt path.

### Task 8: Final verification and review

**Files:**
- Review all modified files in both repositories.

**Interfaces:**
- Produces evidence that all approved requirements are implemented without
  unrelated changes.

- [ ] **Step 1: Run rs-local-files gates**

```bash
cargo test --all-targets
cargo test --doc
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --all-features
cargo +nightly-2026-06-05 fmt -- --check --config-path ../rs-ci/rustfmt.toml
```

- [ ] **Step 2: Run rs-mime against the local crate**

```bash
cargo test --all-targets --config 'patch.crates-io.qubit-local-files.path="../rs-local-files"'
cargo clippy --all-targets --all-features --config 'patch.crates-io.qubit-local-files.path="../rs-local-files"' -- -D warnings
```

- [ ] **Step 3: Inspect changes**

Run `git status --short` and `git --no-pager diff --check` separately in
`rs-local-files` and `rs-mime`, then review the complete diffs. Do not stage or
commit.
