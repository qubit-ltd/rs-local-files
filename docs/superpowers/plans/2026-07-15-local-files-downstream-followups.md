# rs-local-files Downstream Follow-ups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the six approved downstream-informed correctness, diagnostics, documentation, API-evolution, and maintainability follow-ups.

**Architecture:** Keep the synchronous local-filesystem boundary and all public call paths. Record newly created atomic-write parents for bottom-up directory synchronization, make copy staging cleanup explicit in structured errors, harden public enums with `#[non_exhaustive]`, and split the copy-file pipeline into private phases.

**Tech Stack:** Rust 2024, Rust 1.94, standard-library filesystem APIs, existing `log` dependency, external integration tests under `tests/local`.

## Global Constraints

- Preserve all pre-existing dirty-worktree changes.
- Do not add async, VFS, glob, watch, or generic callback-staging APIs.
- Do not add dependencies or new Rust source files.
- Keep tests outside `src/` and follow RED-GREEN for every behavior change.
- Do not add, commit, or push Git changes without explicit authorization.

---

### Task 1: Synchronize newly created atomic-write parent chains

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/internal/path_operations.rs`
- Modify: `src/local/internal/atomic_write.rs`

**Interfaces:**
- Produces: `ensure_parent_path_with_sync_dirs(&Path) -> io::Result<Vec<PathBuf>>`.
- Preserves: `ensure_parent_path(&Path) -> io::Result<()>` for other callers.

- [x] Add a Unix regression that changes an intermediate newly created directory to execute-only inside the atomic callback and expects `SyncParentDirectory` after commit.
- [x] Run the exact test and confirm it fails because only the immediate parent is synchronized.
- [x] Record directories observed missing before creating the parent chain; concurrent creators may cause safe extra synchronization.
- [x] Synchronize the destination parent, then each newly created directory's parent from deepest to shallowest.
- [x] Run all atomic-write tests.

### Task 2: Expose recursive-copy staging cleanup failures

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Modify: `src/local/internal/staged_file.rs`
- Modify: `src/local/internal/copy_dir.rs`
- Modify: `src/local/local_copy_dir_error.rs`
- Modify: `src/local/local_copy_dir_stage.rs`

**Interfaces:**
- Produces: `LocalCopyDirError::{temporary_path, cleanup_error}`.
- Produces: `LocalCopyDirStage::CleanupTemporaryFile`.

- [x] Add a Linux lease-coordinated regression that removes destination write permission after staging and asserts the staging path and secondary cleanup error are retained.
- [x] Run the exact test and confirm the new fields are absent.
- [x] Add explicit `StagedFile::cleanup`, retaining the armed path on failure and warning from `Drop`.
- [x] Convert every post-staging copy error and skip path to explicit cleanup handling while preserving the primary error source.
- [x] Run recursive-copy and structured-error tests.

### Task 3: Correct public documentation contracts

**Files:**
- Modify: `src/local/local_temp_dir.rs`
- Modify: `src/local/local_files.rs`
- Modify: `src/local/local_copy_dir_options.rs`
- Modify: `README.md`, `README.zh_CN.md`
- Modify: `doc/user_guide.md`, `doc/user_guide.zh_CN.md`

- [x] Describe `child_path` as lexical-only and distinguish it from open/ensure child helpers.
- [x] Document atomic-write ancestor synchronization and Unix new-file mode `0o600`.
- [x] Document recursive-copy Unix defaults `0o600`/`0o700` and `preserve_permissions` behavior.
- [x] Document staging cleanup diagnostics and best-effort fallback logging.
- [x] Run doctests and rustdoc warnings through the crate CI wrapper.

### Task 4: Harden public enums for future variants

**Files:**
- Modify: `src/local/local_file_reader.rs`
- Modify: `src/local/local_file_writer.rs`
- Modify: `src/local/local_atomic_write_stage.rs`
- Modify: `src/local/local_copy_dir_stage.rs`
- Modify: affected external tests.

- [x] Add compile-fail examples demonstrating that downstream exhaustive matches are intentionally rejected.
- [x] Run doctests and confirm RED because the matches currently compile.
- [x] Add `#[non_exhaustive]` to all four enums and migrate local variant checks to stable query methods where appropriate.
- [x] Run reader, writer, stage, and doctests.

### Task 5: Split the recursive-copy regular-file pipeline

**Files:**
- Modify: `src/local/internal/copy_dir.rs`

- [x] Preserve the current green copy test baseline.
- [x] Extract focused staging and commit helpers without changing public signatures.
- [x] Keep error stage, partial statistics, conflict, and type-conflict semantics unchanged.
- [x] Run all recursive-copy tests after refactoring.

### Task 6: Verify the crate and downstream

**Files:**
- Inspect: all modified files and public re-exports.

- [x] Run `./align-ci.sh` and inspect its changes.
- [x] Run `./ci-check.sh` and record every stage.
- [x] Run exactly `./coverage.sh json` only if CI reports a coverage threshold failure.
- [x] Run `cargo +1.94.0 test --all-features --verbose` in `../rs-mime`; record the pre-existing duplicate-`qubit-datatype` failure and verify with a CLI-only crates.io patch.
- [x] Run `git --no-pager diff --check`, inspect the full diff, and verify every approved item against this plan.
