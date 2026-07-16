# rs-local-files CWD Stability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep long-lived relative-path resources bound to their creation location, split the oversized `LocalFiles` test suite by responsibility, and compile `libc` only on Linux.

**Architecture:** Preserve caller-facing generated paths while storing a second, absolute operational path inside temporary resource guards. Resolve the atomic destination once at construction and use that frozen path for every later filesystem operation while retaining the requested path in structured errors.

**Tech Stack:** Rust 2024, Rust 1.94, standard-library filesystem APIs, external integration tests.

## Global Constraints

- Preserve all public method signatures and caller-visible relative path spelling.
- Add regression tests before production edits and observe the intended failure.
- Keep all tests under `tests/` and preserve the `local_tests` entry point.
- Do not commit, push, or modify downstream repositories.

---

### Task 1: Reproduce current-directory drift

**Files:**
- Modify: `tests/local/local_atomic_writer_tests.rs`
- Modify: `tests/local/local_temp_file_tests.rs`
- Modify: `tests/local/local_temp_dir_tests.rs`

**Interfaces:**
- Consumes: `CurrentDirGuard`, `CURRENT_DIR_LOCK`, existing public resource APIs.
- Produces: regressions proving creation-time relative paths survive a later `set_current_dir`.

- [ ] Add one regression per long-lived resource. Each test creates the resource from a relative directory under cwd A, switches to cwd B, exercises or drops it, and checks the operation still affected cwd A.
- [ ] Run each exact test with `cargo +1.94.0 test --test local_tests <exact-test-path> -- --exact`.
- [ ] Confirm each test fails because the old implementation resolves its stored relative path against cwd B.

### Task 2: Freeze operational paths

**Files:**
- Modify: `src/local/internal/path_operations.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/local_atomic_writer.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`

**Interfaces:**
- Produces: `pub(crate) fn absolute_path(path: &Path) -> io::Result<PathBuf>`.
- Preserves: `LocalTempFile::path`, `LocalTempFile::keep`, `LocalTempDir::path`, `LocalTempDir::child_path`, and `LocalTempDir::keep` return caller-facing spelling.

- [ ] Add a documented lexical absolute-path helper using `std::path::absolute`.
- [ ] Add private `operation_path` state to temporary file and directory guards; create through an absolute parent, but derive and retain the caller-facing generated path.
- [ ] Route metadata, child operations, cleanup, persistence source moves, and `Drop` through the frozen operational path. Disarm both path fields together only after ownership is released.
- [ ] Resolve the atomic destination once in `LocalAtomicWriter::new`; use it for preparation, replacement, and synchronization while using the requested path for error context.
- [ ] Re-run the three exact regressions and the complete temporary-resource and atomic-writer test modules.

### Task 3: Split `LocalFiles` tests by responsibility

**Files:**
- Modify: `tests/local/local_files_tests.rs`
- Create: focused child modules under `tests/local/local_files_tests/`

**Interfaces:**
- Preserves: the existing `local::local_files_tests::*` discovery hierarchy through module declarations.

- [ ] Move atomic wrapper, basic I/O, path operation, and recursive-copy groups into focused child modules without changing assertions.
- [ ] Give every new Rust file the repository copyright header and explicit imports.
- [ ] Run `cargo +1.94.0 test --test local_tests local::local_files_tests`.

### Task 4: Scope the Linux dependency

**Files:**
- Modify: `Cargo.toml`

**Interfaces:**
- Preserves: Linux `libc` availability for `file_move.rs`; removes it from non-Linux dependency resolution.

- [ ] Move `libc = "0.2"` from `[dependencies]` to `[target.'cfg(target_os = "linux")'.dependencies]`.
- [ ] Confirm the Linux build and tests still resolve the dependency.

### Task 5: Verify and review

**Files:**
- Inspect: all modified files and generated alignment changes.

- [ ] Run `./align-ci.sh`.
- [ ] Run `./ci-check.sh`.
- [ ] Run `./coverage.sh json` only if CI reports coverage below its threshold.
- [ ] Inspect `git --no-pager diff`, verify no unrelated changes, and report exact command results and remaining platform risk.
