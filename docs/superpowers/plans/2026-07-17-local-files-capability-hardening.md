# Local Files Capability Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the remaining descriptor-state, representation, accounting, guard-usage, containment, and copy-module weaknesses from `qubit-local-files`.

**Architecture:** Existing path-based APIs keep their public paths, while reader and writer representations become opaque. A new strongly typed rooted subsystem uses an open Unix directory descriptor as its only authority and returns `Unsupported` on targets without a proven safe backend. Recursive copy keeps its facade but moves traversal, destination, staging, and error responsibilities into focused internal modules.

**Tech Stack:** Rust 1.94, standard library, existing `libc` dependency on Unix, external integration tests, repository CI scripts.

## Global Constraints

- Work in place on the existing `dev-starfish` branch; do not create a worktree.
- Breaking API changes are allowed; prefer strong types and one authoritative state.
- Do not widen production visibility solely for tests.
- Preserve all existing public paths except the intentionally removed reader and writer enum variants.
- Rooted operations must never fall back to check-then-absolute-path authority.
- Unsupported secure platform operations return `io::ErrorKind::Unsupported`.
- Do not commit unless the user explicitly requests a new commit operation after implementation.

---

### Task 1: Clear transient non-blocking descriptor state

**Files:**
- Modify: `tests/local/local_file_reader_tests.rs`
- Modify: `tests/local/local_file_writer_tests.rs`
- Modify: `src/local/internal/file_io.rs`

**Interfaces:**
- Consumes: `LocalFiles::open_reader`, `LocalFiles::open_writer`.
- Produces: returned Unix regular-file descriptors with `O_NONBLOCK` cleared while all other status flags are preserved.

- [ ] Add Linux integration tests that open unique files, locate their live descriptor through `/proc/self/fd`, parse `/proc/self/fdinfo/<fd>/flags` as octal, and assert `flags & libc::O_NONBLOCK == 0` for reader and writer.
- [ ] Run each focused test and confirm it fails because the returned descriptor still contains `O_NONBLOCK`.
- [ ] Add a documented Unix `clear_nonblocking(&File) -> io::Result<()>` helper using `fcntl(F_GETFL)` followed by `fcntl(F_SETFL, flags & !O_NONBLOCK)`. Explain that `O_NONBLOCK` is only an anti-FIFO-race open flag and must not leak into the public wrapper's observable descriptor state.
- [ ] Call the helper only after handle metadata proves that the opened object is a regular file; contextualize failures with the operation path.
- [ ] Re-run the two focused tests and the existing reader/writer test modules.

### Task 2: Make reader and writer representations opaque

**Files:**
- Create: `src/local/internal/local_file_reader_inner.rs`
- Create: `src/local/internal/local_file_writer_inner.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/local_file_reader.rs`
- Modify: `src/local/local_file_writer.rs`
- Modify: `tests/local/local_file_reader_tests.rs`
- Modify: `tests/local/local_file_writer_tests.rs`

**Interfaces:**
- Consumes: `FileBuffering`, `File`, `BufReader<File>`, `BufWriter<File>`.
- Produces: opaque `pub struct LocalFileReader` and `pub struct LocalFileWriter` preserving `Read`, `Seek`, `Write`, `close`, `sync_all`, `sync_data`, and `is_buffered`.

- [ ] Add compile-fail doctests that attempt to name `LocalFileReader::Unbuffered` and `LocalFileWriter::Buffered`; confirm the current enums make those snippets compile or fail for the old non-exhaustive reason rather than missing variants.
- [ ] Define one private inner enum per new internal file, with unbuffered and buffered variants and narrowly visible delegation methods.
- [ ] Replace each public enum with a documented struct containing exactly one private inner-enum field.
- [ ] Delegate all existing trait and inherent behavior through the private inner enum without adding raw-handle or `File` accessors.
- [ ] Update representation-sensitive tests to assert only public behavior and run the reader/writer test modules plus doctests.

### Task 3: Protect cleanup guards with type-level `must_use`

**Files:**
- Modify: `src/local/local_atomic_writer.rs`
- Modify: `src/local/local_temp_file.rs`
- Modify: `src/local/local_temp_dir.rs`
- Modify: `src/local/internal/staged_file.rs`

**Interfaces:**
- Produces: type-level warning contracts for all public and internal guards whose immediate drop discards staged work or removes a resource.

- [ ] Add public compile-fail examples using `#![deny(unused_must_use)]` that discard successful `LocalAtomicWriter`, `LocalTempFile`, and `LocalTempDir` values after extracting them from `Result`; confirm each snippet compiles before annotations.
- [ ] Add reasoned type-level `#[must_use = "..."]` attributes to the three public guards and internal `StagedFile`.
- [ ] Run crate doctests and confirm the public examples now fail compilation for discarded guard values.

### Task 4: Add a validated rooted path and anchored ordinary I/O

**Files:**
- Create: `src/local/local_relative_path.rs`
- Create: `src/local/local_root.rs`
- Create: `src/local/internal/rooted_file_io.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Create: `tests/local/local_relative_path_tests.rs`
- Create: `tests/local/local_root_tests.rs`
- Modify: `tests/local/mod.rs`

**Interfaces:**
- Produces: `LocalRelativePath::new`, `LocalRelativePath::as_path`, `LocalRoot::open`, `LocalRoot::path`, `LocalRoot::open_reader`, and `LocalRoot::open_writer`.
- Consumes: opaque reader/writer constructors and existing read/write option types.

- [ ] Write path tests for empty, absolute, root, prefix where available, `.`, `..`, and embedded NUL rejection, plus normal Unicode multi-component acceptance; run them and confirm missing-type failures.
- [ ] Implement `LocalRelativePath` as one validated owned `PathBuf`, rejecting every component except `Component::Normal` and rejecting target-platform NUL values.
- [ ] Write rooted Unix tests for ordinary read/write, missing-parent creation, intermediate and final symlink denial, root rename after open, and non-Unix `Unsupported`; run them and confirm missing-type failures.
- [ ] Implement `LocalRoot` with a diagnostic absolute path and, on Unix, an open directory `File`. Open the root with `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`, then verify directory metadata through the handle.
- [ ] Implement Unix traversal by cloning the root descriptor and opening each intermediate component with `openat(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)`. Parent creation uses `mkdirat` and then the same no-follow open-and-verify path.
- [ ] Implement final reader/writer opens with `openat`, `O_NOFOLLOW`, `O_CLOEXEC`, and transient `O_NONBLOCK`; verify regular-file metadata through the opened handle and then clear `O_NONBLOCK`.
- [ ] Return `Unsupported` from the rooted backend on non-Unix targets rather than constructing an absolute descendant path.
- [ ] Run both new test modules and existing local file I/O tests.

### Task 5: Add rooted atomic replacement

**Files:**
- Create: `src/local/local_root_atomic_writer.rs`
- Create: `src/local/internal/rooted_staged_file.rs`
- Create: `src/local/internal/rooted_atomic_write.rs`
- Modify: `src/local/internal/mod.rs`
- Modify: `src/local/local_root.rs`
- Modify: `src/local/mod.rs`
- Modify: `src/lib.rs`
- Create: `tests/local/local_root_atomic_writer_tests.rs`
- Modify: `tests/local/mod.rs`

**Interfaces:**
- Produces: `LocalRoot::begin_atomic_write`, `LocalRootAtomicWriter: Write`, `commit`, and `abort`.
- Consumes: anchored parent descriptor traversal and `LocalAtomicWriteError` stages.

- [ ] Write Unix tests for commit replacement, abort cleanup, drop cleanup, permission preservation, root rename before commit, final symlink replacement containment, and parent durability error state; run them and confirm missing-method/type failures.
- [ ] Add a private rooted staging guard that owns the parent directory descriptor, staging entry name, and optional staging file handle; cleanup uses `unlinkat` and never a reconstructed authority path.
- [ ] Create staging files with `openat(O_CREAT | O_EXCL | O_RDWR | O_CLOEXEC, 0o600)` and bounded random-name retries in the destination parent descriptor.
- [ ] Inspect an existing final entry with `fstatat(AT_SYMLINK_NOFOLLOW)`, reject links and non-regular entries, and retain ordinary-file permissions for commit.
- [ ] Implement commit as permission application, staging `sync_all`, data-handle close, same-parent `renameat`, parent-directory `sync_all`, then cleanup disarm. Preserve `committed = true` only for post-rename durability failures.
- [ ] Implement explicit abort and `Drop` cleanup through `unlinkat`; add type-level `must_use` to `LocalRootAtomicWriter` and a compile-fail doctest.
- [ ] Return structured `Unsupported` errors on non-Unix targets and run rooted atomic tests plus existing atomic-writer tests.

### Task 6: Split recursive copy and reject counter overflow

**Files:**
- Replace module file: `src/local/internal/copy_dir.rs` with `src/local/internal/copy_dir/mod.rs`
- Create: `src/local/internal/copy_dir/error.rs`
- Create: `src/local/internal/copy_dir/traversal.rs`
- Create: `src/local/internal/copy_dir/destination.rs`
- Create: `src/local/internal/copy_dir/staged_copy.rs`
- Create: `src/local/internal/copy_dir/source.rs`
- Modify: `src/local/local_copy_dir_stats.rs`
- Modify: `tests/local/local_copy_dir_stats_tests.rs`
- Modify: `tests/local/local_files_tests/copy_dir_tests.rs`

**Interfaces:**
- Preserves: `internal::copy_dir_all_with_paths` and every public recursive-copy API and error type.
- Produces: checked `LocalCopyDirStats` mutations that return contextual `InvalidData` errors instead of saturated counts.

- [ ] Record that a practical end-to-end overflow fixture would require copying more than `u64::MAX` files or bytes, while external tests cannot call private accounting helpers; do not widen the public API or add a production test seam solely for this case.
- [ ] Replace every saturating single-field update with a documented private checked-update helper that maps overflow to `InvalidData` containing the overflowing field name and the source/destination operation context.
- [ ] Run the existing copy-statistics and recursive-copy tests to protect all reachable accounting behavior.
- [ ] Protect the current recursive-copy behavior by running its full existing test module before structural changes.
- [ ] Move only error construction/context helpers to `error.rs`; source metadata and containment checks to `source.rs`; destination conflict preparation/removal to `destination.rs`; staging and commit to `staged_copy.rs`; recursive enumeration and symlink dispatch to `traversal.rs`; retain the public internal facade in `mod.rs`.
- [ ] Keep helper visibility at `pub(super)` or private, use explicit imports in every file, preserve function contracts, and apply the repository copyright/Rustdoc/inline rules to each moved item.
- [ ] Re-run the complete recursive-copy test module and stats tests.

### Task 7: Re-audit and verify the complete change

**Files:**
- Inspect all changed Rust, test, documentation, and manifest files.
- Modify only in-scope files when verification finds an issue.

**Interfaces:**
- Produces: CI-equivalent verified changes and a refreshed code knowledge graph.

- [ ] Re-index `/home/starfish/working/qubit/rust-common` with codebase-memory and trace the new root API plus copy facade to confirm intended call boundaries.
- [ ] Run `./align-ci.sh` from `rs-local-files`, inspect every formatter/alignment edit, and rerun it after any correction.
- [ ] Run `./ci-check.sh` from `rs-local-files`; if and only if it reports coverage below threshold, run exactly `./coverage.sh json`, add meaningful in-scope tests, and repeat the required sequence.
- [ ] Run `cargo +1.94.0 test --manifest-path ../rs-mime/Cargo.toml` to verify the direct downstream consumer after the breaking opaque-reader/writer change.
- [ ] Review `git diff --check`, `git status --short`, and the complete diff. Report actual commands, results, unresolved risks, and unchecked platforms without committing.
