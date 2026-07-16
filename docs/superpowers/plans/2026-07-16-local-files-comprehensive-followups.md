# rs-local-files Comprehensive Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Correct the approved atomic-write and path-validation defects, add a minimal streaming atomic writer, encapsulate configuration fields in a `0.5.0` release, document copy race boundaries, and finish the identified Rust style corrections.

**Architecture:** Move the complete atomic lifecycle into a public `LocalAtomicWriter` that owns the existing private `StagedFile`; keep `LocalFiles::atomic_write` and `atomic_write_with` as thin wrappers. Preserve the crate's synchronous standard-library boundary, use getters for private configuration state, and keep adversarial root containment outside this path-based crate.

**Tech Stack:** Rust 2024, Rust 1.94.0, `std::fs`, existing `getrandom`/`libc`/`log` dependencies, external integration tests, rustdoc compile-fail tests, Unix/Windows/macOS conditional code.

## Global Constraints

- Keep Rust `1.94` and edition `2024`; do not add runtime dependencies or async APIs.
- `LocalAtomicWriter` implements `Write`, `commit`, and `abort`; it does not implement `Seek` or expose its canonical `File`.
- Preserve the call signatures of `LocalFiles::atomic_write` and `atomic_write_with`.
- Make only configuration fields private; statistics and structured-error fields remain public.
- Keep tests under `tests/`; do not add inline test modules or production visibility solely for tests.
- For every behavior change, add the regression first, run it to observe the intended failure, then implement the smallest passing change.
- Preserve unrelated worktree changes. Commit `rs-local-files`, `rs-mime`, and `rs-magika` changes separately within their own repositories.
- Use repository-standard `# Parameters` Rustdoc headings.

---

### Task 1: Do not inherit permissions through a destination symlink

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/internal/atomic_write.rs`

**Interfaces:**
- Preserves: `LocalFiles::atomic_write` and `LocalFiles::atomic_write_with`.
- Produces: only an actual regular-file destination entry donates permissions.

- [ ] **Step 1: Add the Unix regression**

Add beside the existing symlink replacement test:

```rust
#[cfg(unix)]
#[test]
fn test_atomic_write_does_not_inherit_symlink_target_permissions() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-symlink-permissions");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, b"target").expect("target should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o777))
        .expect("target permissions should be set");
    symlink(&target, &link).expect("symlink should be created");

    LocalFiles::atomic_write(&link, b"replacement")
        .expect("symlink path should be replaced");

    let replacement_mode =
        fs::metadata(&link).unwrap().permissions().mode() & 0o777;
    let target_mode =
        fs::metadata(&target).unwrap().permissions().mode() & 0o777;
    assert_eq!(0, replacement_mode & 0o177);
    assert_eq!(0o777, target_mode);
    assert_eq!(b"target", fs::read(&target).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}
```

- [ ] **Step 2: Run the exact test and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write_does_not_inherit_symlink_target_permissions -- --exact
```

Expected: FAIL because the replacement mode inherits `0o777` from the target.

- [ ] **Step 3: Inspect the destination entry without following links**

Replace the permission inspection body with:

```rust
fn existing_file_permissions(path: &Path) -> Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file() && !metadata.file_type().is_symlink() =>
        {
            Ok(Some(metadata.permissions()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(add_path_context(error, "read destination metadata", path))
        }
    }
}
```

- [ ] **Step 4: Run focused atomic tests and commit**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write
git add src/local/internal/atomic_write.rs tests/local/local_files_tests.rs
git commit -m "fix: avoid inheriting permissions through symlinks"
```

Expected: all matching atomic-write tests pass.

### Task 2: Preserve the canonical staging handle for callbacks

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/internal/atomic_write.rs`
- Modify: `src/local/local_files.rs`

**Interfaces:**
- Preserves: `FnOnce(&mut File) -> io::Result<()>` public callback shape.
- Produces: `atomic_write_with_path<F>(path, write)` with generic `FnOnce`.

- [ ] **Step 1: Replace the handle-swapping sync-error test with the invariant regression**

Replace `test_atomic_write_with_returns_temporary_sync_error` with:

```rust
#[cfg(target_os = "linux")]
#[test]
fn test_atomic_write_with_keeps_canonical_staging_handle() {
    let dir = temp_dir("atomic-canonical-handle");
    let path = dir.join("out.txt");

    LocalFiles::atomic_write_with(&path, |file| {
        file.write_all(b"committed")?;
        *file = fs::OpenOptions::new()
            .write(true)
            .open("/dev/full")?;
        Ok(())
    })
    .expect("replacing the callback handle must not replace the staging handle");

    assert_eq!(b"committed", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}
```

Remove `test_atomic_write_with_returns_permission_preservation_error`, which
also depends on redirecting the staging handle to an unrelated file.

- [ ] **Step 2: Run the exact regression and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write_with_keeps_canonical_staging_handle -- --exact
```

Expected: FAIL with `SyncTemporaryFile` because current code synchronizes
`/dev/full`.

- [ ] **Step 3: Make the internal callback `FnOnce` and pass a clone**

Use this signature and callback block:

```rust
pub(crate) fn atomic_write_with_path<F>(
    path: &Path,
    write: F,
) -> std::result::Result<(), LocalAtomicWriteError>
where
    F: FnOnce(&mut File) -> Result<()>,
{
    // existing preparation remains unchanged
    let mut callback_file = match staged_file.file().try_clone() {
        Ok(file) => file,
        Err(source) => {
            return Err(atomic_error_with_staging(
                LocalAtomicWriteStage::WriteTemporaryFile,
                path,
                source,
                &mut staged_file,
            ));
        }
    };
    if let Err(source) = write(&mut callback_file) {
        return Err(atomic_error_with_staging(
            LocalAtomicWriteStage::WriteTemporaryFile,
            path,
            source,
            &mut staged_file,
        ));
    }
    drop(callback_file);
    // permission preservation, sync, replace, and parent sync remain unchanged
}
```

Pass closures by value from `atomic_write_bytes_path` and simplify the public
wrapper to:

```rust
atomic_write_with_path(path.as_ref(), write)
```

- [ ] **Step 4: Run atomic tests and commit**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write
git add src/local/internal/atomic_write.rs src/local/local_files.rs tests/local/local_files_tests.rs
git commit -m "fix: retain the canonical atomic staging handle"
```

### Task 3: Propagate existing-prefix inspection errors

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/internal/path_operations.rs`

**Interfaces:**
- Preserves: `copy_dir_all_with` API.
- Produces: invalid destination inspection fails before source traversal.

- [ ] **Step 1: Add the Unix ordering regression**

```rust
#[cfg(unix)]
#[test]
fn test_copy_dir_all_with_validates_invalid_destination_before_missing_source() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let dir = temp_dir("copy-invalid-destination-first");
    let src = dir.join("missing-source");
    let dst = dir.join(OsString::from_vec(b"dst\0invalid".to_vec()));

    let error = LocalFiles::copy_dir_all_with(
        &src,
        &dst,
        LocalCopyDirOptions::default(),
    )
    .expect_err("invalid destination should fail before source inspection");

    assert_eq!(LocalCopyDirStage::PrepareDestination, error.stage);
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    fs::remove_dir_all(dir).unwrap();
}
```

- [ ] **Step 2: Run the exact test and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_copy_dir_all_with_validates_invalid_destination_before_missing_source -- --exact
```

Expected: FAIL because current `Path::exists` hides the NUL error and source
inspection reports `InspectSource`/`NotFound` first.

- [ ] **Step 3: Use `try_exists` throughout the ancestor walk**

```rust
pub(super) fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    if path.try_exists()? {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = path.to_path_buf();
    while !current.try_exists()? {
        if let Some(name) = current.file_name() {
            missing.push(name.to_os_string());
        } else {
            break;
        }
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                current = parent.to_path_buf();
            }
            _ => {
                current = env::current_dir()?;
                break;
            }
        }
    }
    let mut canonical = fs::canonicalize(current)?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}
```

- [ ] **Step 4: Run copy tests and commit**

```bash
cargo +1.94.0 test --test local_tests local::local_files_tests::test_copy_dir_all_with
git add src/local/internal/path_operations.rs tests/local/local_files_tests.rs
git commit -m "fix: preserve existing-prefix inspection errors"
```

### Task 4: Document recursive-copy concurrency boundaries

**Files:**
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_copy_dir_options.rs`
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`

**Interfaces:**
- Produces: a public contract that path checks are not an adversarial sandbox.

- [ ] **Step 1: Add the public Rustdoc contract**

Add this paragraph to both `copy_dir_all_with` and the
`LocalCopyDirOptions::follow_symlinks` field documentation:

```rust
/// Source inspection, source opening, destination reinspection, and
/// destructive replacement are separate path-based operations. The symbolic
/// link policy prevents ordinary accidental traversal, but it is not a
/// sandbox boundary when an untrusted actor can mutate either tree
/// concurrently. Use descriptor- or capability-relative filesystem APIs when
/// containment must resist concurrent path replacement.
```

- [ ] **Step 2: Add matching English and Chinese guide text**

English text:

```markdown
Source checks, source opens, destination rechecks, and destructive replacement
are separate path-based operations. The symlink policy prevents accidental
traversal; it is not an attacker-resistant sandbox when another actor can
mutate either tree concurrently.
```

Chinese text:

```markdown
源路径检查、源文件打开、目标复查和破坏性替换是彼此分离的 path-based
操作。symlink 策略用于避免普通的意外穿越；当其他参与者能够并发修改任一
目录树时，它不是可抵御攻击者的 sandbox 边界。
```

- [ ] **Step 3: Verify docs and commit**

```bash
cargo +1.94.0 test --doc
git add src/local/local_files.rs src/local/local_copy_dir_options.rs README.md README.zh_CN.md doc/user_guide.md doc/user_guide.zh_CN.md
git commit -m "docs: define recursive-copy race boundaries"
```

### Task 5: Add the streaming `LocalAtomicWriter`

**Files:**
- Create: `src/local/local_atomic_writer.rs`
- Create: `tests/local/local_atomic_writer_tests.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_atomic_write_stage.rs`
- Modify: `src/local/internal/mod.rs`
- Delete: `src/local/internal/atomic_write.rs`
- Modify: `tests/local/mod.rs`

**Interfaces:**
- Produces: `LocalFiles::begin_atomic_write<P>(P) -> Result<LocalAtomicWriter, LocalAtomicWriteError>`.
- Produces: `LocalAtomicWriter: Write + Send`.
- Produces: `commit(self)` and `abort(self)` returning structured atomic errors.
- Preserves: existing atomic convenience APIs.

- [ ] **Step 1: Add public lifecycle tests before exporting the type**

Create `tests/local/local_atomic_writer_tests.rs`:

```rust
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;

use qubit_local_files::{
    LocalAtomicWriter,
    LocalFiles,
};

use super::test_support::{
    count_atomic_temp_files,
    fs,
    temp_dir,
};

fn assert_send<T: Send>() {}

#[test]
fn test_local_atomic_writer_is_send() {
    assert_send::<LocalAtomicWriter>();
}

#[test]
fn test_local_atomic_writer_commits_written_contents() {
    let dir = temp_dir("atomic-writer-commit");
    let path = dir.join("out.txt");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    writer.write_all(b"committed").expect("contents should write");
    assert!(!path.exists());
    writer.commit().expect("atomic writer should commit");
    assert_eq!(b"committed", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_abort_preserves_destination() {
    let dir = temp_dir("atomic-writer-abort");
    let path = dir.join("out.txt");
    fs::write(&path, b"original").unwrap();
    let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
    writer.write_all(b"replacement").unwrap();
    writer.abort().expect("atomic writer should abort");
    assert_eq!(b"original", fs::read(&path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_local_atomic_writer_drop_removes_staging_file() {
    let dir = temp_dir("atomic-writer-drop");
    let path = dir.join("out.txt");
    {
        let mut writer = LocalFiles::begin_atomic_write(&path).unwrap();
        writer.write_all(b"discarded").unwrap();
    }
    assert!(!path.exists());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}
```

Register `mod local_atomic_writer_tests;` in `tests/local/mod.rs`.

- [ ] **Step 2: Run the new test module and confirm RED**

```bash
cargo +1.94.0 test --test local_tests local::local_atomic_writer_tests
```

Expected: compilation fails because `LocalAtomicWriter` and
`begin_atomic_write` do not exist.

- [ ] **Step 3: Move atomic state and protocol into the new type**

Create `src/local/local_atomic_writer.rs` with this public shape and ownership:

```rust
pub struct LocalAtomicWriter {
    path: PathBuf,
    parent_dirs_to_sync: Vec<PathBuf>,
    existing_permissions: Option<fs::Permissions>,
    staged_file: StagedFile,
}

impl LocalAtomicWriter {
    pub(crate) fn new(path: &Path) -> Result<Self, LocalAtomicWriteError>;

    pub fn commit(self) -> Result<(), LocalAtomicWriteError>;

    pub fn abort(mut self) -> Result<(), LocalAtomicWriteError>;

    pub(crate) fn write_bytes(
        mut self,
        bytes: &[u8],
    ) -> Result<(), LocalAtomicWriteError>;

    pub(crate) fn write_with<F>(
        mut self,
        write: F,
    ) -> Result<(), LocalAtomicWriteError>
    where
        F: FnOnce(&mut File) -> io::Result<()>;
}

impl Write for LocalAtomicWriter {
    #[inline]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.staged_file.file_mut().write(buffer)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.staged_file.file_mut().flush()
    }
}
```

Move the constants and helper functions from `internal/atomic_write.rs` into
this file. `new` performs parent preparation, permission inspection, and
same-directory staging. `write_with` clones the canonical file before invoking
the callback. `commit` destructures `self`, applies permissions, syncs, closes,
replaces, disarms, and syncs the parent chain. `abort` calls `cleanup` and maps
failure to `CleanupTemporaryFile` with `committed = false`.

Change internal visibility/reexports only for the primitives consumed by this
sibling module: `add_path_context`, `ensure_parent_path_with_sync_dirs`,
`parent_dir_for`, and `sync_parent_dir` remain crate-private implementation
details.

- [ ] **Step 4: Export the type and add the namespace constructor**

In `src/local/mod.rs` add:

```rust
mod local_atomic_writer;
pub use local_atomic_writer::LocalAtomicWriter;
```

In `src/lib.rs` export `LocalAtomicWriter`. In `LocalFiles` add:

```rust
#[inline(always)]
pub fn begin_atomic_write<P>(
    path: P,
) -> std::result::Result<LocalAtomicWriter, LocalAtomicWriteError>
where
    P: AsRef<Path>,
{
    LocalAtomicWriter::new(path.as_ref())
}
```

Rewrite the convenience methods as:

```rust
Self::begin_atomic_write(path)?.write_bytes(bytes.as_ref())
```

and:

```rust
Self::begin_atomic_write(path)?.write_with(write)
```

Delete `internal/atomic_write.rs` and its reexports after all callers move.

- [ ] **Step 5: Add the explicit cleanup stage**

Add to `LocalAtomicWriteStage` before `SyncParentDirectory`:

```rust
/// Explicitly removing an aborted temporary file failed.
CleanupTemporaryFile,
```

Update its compile-fail example and stage tests.

- [ ] **Step 6: Run focused and complete local tests, then commit**

```bash
cargo +1.94.0 test --test local_tests local::local_atomic_writer_tests
cargo +1.94.0 test --test local_tests local::local_files_tests::test_atomic_write
cargo +1.94.0 test --test local_tests
git add src tests
git commit -m "feat: add streaming atomic file writer"
```

### Task 6: Privatize configuration fields and release `0.5.0`

**Files:**
- Modify: `src/local/file_read_options.rs`
- Modify: `src/local/file_write_options.rs`
- Modify: `src/local/local_copy_dir_options.rs`
- Modify: `src/local/local_persist_options.rs`
- Modify: internal consumers in `src/local/**`
- Modify: affected tests under `tests/local/**`
- Modify: `Cargo.toml`, `Cargo.lock`
- Modify: `../rs-mime/Cargo.toml`, `../rs-mime/Cargo.lock`
- Modify: `../rs-magika/Cargo.lock`

**Interfaces:**
- Preserves: all current getters, builders, defaults, and derives.
- Breaks intentionally: external direct field access.
- Produces: `qubit-local-files 0.5.0` and `rs-mime` requirement `0.5`.

- [ ] **Step 1: Add compile-fail direct-mutation examples**

Add an example like this to each configuration type, using that type's field:

```rust
/// ```compile_fail
/// use qubit_local_files::FileWriteOptions;
///
/// let mut options = FileWriteOptions::default();
/// options.create_parent = true;
/// ```
```

For the other types use `buffering`, `follow_symlinks`, and `overwrite`.

- [ ] **Step 2: Run doctests and confirm RED**

```bash
cargo +1.94.0 test --doc
```

Expected: FAIL because the compile-fail examples currently compile.

- [ ] **Step 3: Remove `pub` and use getters from sibling modules**

The resulting declarations are:

```rust
pub struct FileReadOptions {
    buffering: FileBuffering,
}

pub struct FileWriteOptions {
    create_parent: bool,
    mode: FileWriteMode,
    buffering: FileBuffering,
}

pub struct LocalCopyDirOptions {
    conflict: LocalCopyConflictPolicy,
    type_conflict: LocalCopyTypeConflictPolicy,
    follow_symlinks: bool,
    preserve_permissions: bool,
}

pub struct LocalPersistOptions {
    overwrite: bool,
}
```

Replace sibling-module reads with `buffering()`, `creates_parent()`, `mode()`,
`conflict_policy()`, `type_conflict_policy()`, `follows_symlinks()`,
`preserves_permissions()`, and `overwrites()`.

- [ ] **Step 4: Verify the API change locally**

```bash
cargo +1.94.0 test --doc
cargo +1.94.0 test --test local_tests
```

Expected: doctests and integration tests pass.

- [ ] **Step 5: Bump manifests and refresh each repository lockfile**

Set `rs-local-files/Cargo.toml` to `version = "0.5.0"` and
`rs-mime/Cargo.toml` to `qubit-local-files ... version = "0.5"`. Then run:

```bash
cargo +1.94.0 check
cargo +1.94.0 check --manifest-path ../rs-mime/Cargo.toml
cargo +1.94.0 update --manifest-path ../rs-magika/Cargo.toml -p qubit-local-files
```

- [ ] **Step 6: Commit each repository separately**

```bash
git add Cargo.toml Cargo.lock src tests
git commit -m "feat!: encapsulate local file configuration"
git -C ../rs-mime add Cargo.toml Cargo.lock
git -C ../rs-mime commit -m "chore: update qubit-local-files to 0.5"
git -C ../rs-magika add Cargo.lock
git -C ../rs-magika commit -m "chore: update qubit-local-files lock entry"
```

### Task 7: Apply the approved Rust style corrections

**Files:**
- Create: `src/local/internal/file_attribute_tag_info.rs`
- Create: `src/local/internal/file_disposition_info.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/internal/file_move.rs`
- Modify: `src/local/local_filenames.rs`
- Modify: `src/local/local_file_writer.rs`
- Reorder inherent methods in the approved option and error files

**Interfaces:**
- Preserves all public behavior and signatures.

- [ ] **Step 1: Move each Windows FFI type into its own file**

Create one header-documented module per type. The declarations are:

```rust
#[cfg(windows)]
#[repr(C)]
pub(super) struct FileAttributeTagInfo {
    /// Bit mask of `FILE_ATTRIBUTE_*` values.
    pub(super) file_attributes: u32,
    /// Reparse tag, or zero when the object is not a reparse point.
    pub(super) reparse_tag: u32,
}
```

and:

```rust
#[cfg(windows)]
#[repr(C)]
pub(super) struct FileDispositionInfo {
    /// Windows `BOOLEAN` value indicating whether deletion is requested.
    pub(super) delete_file: u8,
}
```

Register and reexport both from `internal/mod.rs`, then import them from
`file_move.rs`.

- [ ] **Step 2: Complete item documentation**

Add concise `///` documentation to every module-level platform constant and
each foreign declaration in `file_move.rs`, plus
`MAX_PORTABLE_FILE_NAME_BYTES` and `RANDOM_NAME_BYTES` in
`local_filenames.rs`. Keep all existing call-site `SAFETY` comments.

- [ ] **Step 3: Reorder constructors and correct inline attributes**

Move `new` and named factories before getters in `FileBuffering`,
`FileReadOptions`, `FileWriteOptions`, `LocalCopyDirOptions`,
`LocalPersistOptions`, `LocalAtomicWriteError`, `LocalCopyDirError`, and
`LocalPersistError`. Add `#[inline]` to `LocalFileWriter::sync_all` and
`sync_data`; change pure `Default -> Self::new()` forwarders to
`#[inline(always)]`.

- [ ] **Step 4: Run formatting/style/host tests and commit**

```bash
cargo +1.94.0 fmt --all
cargo +1.94.0 fmt --all -- --check
./style-check.sh
cargo +1.94.0 test --test local_tests
git add src tests
git commit -m "style: align local-files Rust organization"
```

### Task 8: Update public documentation and verify all repositories

**Files:**
- Modify: `README.md`
- Modify: `README.zh_CN.md`
- Modify: `doc/user_guide.md`
- Modify: `doc/user_guide.zh_CN.md`
- Modify: any generated/aligned CI files changed by `align-ci.sh`

**Interfaces:**
- Documents: `LocalAtomicWriter`, `0.5.0` private fields, symlink permission
  behavior, and unchanged synchronous crate boundary.

- [ ] **Step 1: Add lifecycle examples and migration notes**

Use this example in both guides:

```rust
use std::io::Write;
use qubit_local_files::LocalFiles;

let mut writer = LocalFiles::begin_atomic_write("state.bin")?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Document that dropping or aborting leaves the destination unchanged, that the
writer is `Write` but not `Seek`, and that configuration fields must be changed
through builders in `0.5.0`.

Use this Chinese migration paragraph:

```markdown
`LocalAtomicWriter` 实现 `Write`，但首版不实现 `Seek`。只有 `commit`
成功后目标才会被替换；调用 `abort` 或直接 drop 会保留原目标并清理 staging
文件。自 `0.5.0` 起，配置类型的字段不再公开，调用方必须使用现有 getter、
constructor 和 builder。
```

- [ ] **Step 2: Run the full local validation sequence**

```bash
./align-ci.sh
./style-check.sh
./ci-check.sh
COVERAGE_OPEN_HTML=0 ./coverage.sh json
```

Expected: every command exits zero; coverage remains at or above configured
per-source thresholds.

- [ ] **Step 3: Verify direct and transitive downstreams**

```bash
cargo +1.94.0 test --manifest-path ../rs-mime/Cargo.toml
cargo +1.94.0 check --manifest-path ../rs-magika/Cargo.toml --no-default-features
```

Expected: both commands exit zero with `qubit-local-files 0.5.0` selected.

- [ ] **Step 4: Review diffs and commit final documentation separately**

```bash
git diff --check
git status --short
git add README.md README.zh_CN.md doc .rs-ci .github align-ci.sh ci-check.sh coverage.sh style-check.sh update-submodule.sh
git commit -m "docs: describe streaming atomic writes"
```

Only stage paths that actually changed; do not create an empty commit.
