# rs-local-files Review Follow-ups Design

## Goal

Resolve the correctness and API issues confirmed during the second
`rs-local-files` 0.3 review, split the oversized `local_files.rs`
implementation into focused private modules, document non-transactional and
platform-dependent behavior, and complete the authorized API-hygiene cleanup.

This work remains part of the source-breaking 0.3 release. Compatibility with
the released 0.2 API is not required, but downstream crates in this workspace
must be migrated and verified together.

## Crate Boundary

The crate remains synchronous and limited to concrete local filesystem
operations. It does not depend on `qubit-fs`, add asynchronous APIs, or claim
capability-based filesystem isolation.

`LocalFileReader` and `LocalFileWriter` remain public enums. Their exposed
variants have not caused a correctness or safety problem, downstream crates do
not rely on exhaustive matching, and replacing them with opaque wrappers would
add forwarding code and make access to standard-library handles more complex
without a proportionate benefit.

## Temporary Directory Child Writer Safety

`LocalTempDir::open_child_writer` must reject a final symbolic link whether the
link target exists or is dangling, and whether the target is inside or outside
the temporary directory. The final component is inspected with
`symlink_metadata` before opening. A symbolic link produces
`ErrorKind::InvalidInput`; a regular file remains eligible for opening; a
missing path remains eligible for creation.

An automated Unix regression test creates a dangling link inside the temporary
directory that points to a missing file outside the directory. The test must
first demonstrate that the current implementation creates the outside target,
then pass after the fix and assert that the outside target remains absent.

This check does not remove the existing time-of-check/time-of-use limitation.
The Rustdoc continues to state that child helpers are convenience containment
checks and are not a sandbox against a concurrently mutating untrusted actor.

## Windows Path Conversion

Every path passed to the raw Windows `MoveFileExW` and `CreateFileW` bindings
is converted by one fallible helper. The helper rejects an interior UTF-16 NUL
with `ErrorKind::InvalidInput` before invoking Win32. This prevents a target
such as `target\0ignored` from being interpreted as the shorter `target` path.

Windows-only regressions cover both no-replace persistence and replacement
through atomic write. The tests verify that neither the NUL-prefixed path nor
an existing prefix target is moved or overwritten. The documentation also
states that native move semantics retain their platform path-length limits;
the crate does not silently change relative-path or verbatim-path semantics in
this correction.

## Windows Directory Symlink Removal

`LocalFiles::remove_any` continues to remove symbolic links themselves without
following them. On Windows, `FileTypeExt::is_symlink_dir` selects
`fs::remove_dir` for directory symlinks, while file symlinks use
`fs::remove_file`. Unix keeps using `unlink` through `fs::remove_file` for all
symlinks.

A Windows regression creates a directory symlink, removes it through the
public API, and verifies that the target directory remains. Environments that
do not grant symlink creation privilege report that limitation instead of
changing production behavior.

## Atomic-Write Panic Cleanup

Atomic write and recursive file copy share a private RAII staging-file guard.
The guard owns the open handle and temporary path, removes the uncommitted path
from `Drop`, and is disarmed only after a successful move. This makes callback
unwinding in `LocalFiles::atomic_write_with` close and remove the staging file
without catching or translating the panic.

Rustdoc documents that callback panics propagate. A `catch_unwind` regression
verifies that the destination remains unchanged and no `.atomic-write-*.tmp`
entry is left behind.

## Portable File Name Validation

`LocalFilenames::validate_portable_file_name` defines a name that is
lexically compatible with Windows, Linux, and macOS. Its behavior must be
target-independent: every build applies the union of the three platforms'
rules. Conditional compilation would allow a name accepted on Linux to fail
after transfer to Windows and would contradict the method's portable
contract.

The implementation is logically divided into common component checks and
platform-compatibility checks, but the checks run on all targets. A future API
that validates only the current native platform would be separately named and
could use conditional compilation; that API is outside this work.

The Windows rules are extended to reject `COM\u{00b9}`, `COM\u{00b2}`,
`COM\u{00b3}`, `LPT\u{00b9}`, `LPT\u{00b2}`, and `LPT\u{00b3}`, including
case variants and names with extensions. Existing non-reserved counterexamples
such as `COM0`, `COM10`, and `LPT0` remain valid. Rustdoc links directly to
the Microsoft file-naming documentation, Linux `pathname(7)`, Apple's syscall
file-name documentation, and the Apple File System FAQ.

The validation remains useful even when the native filesystem would reject an
invalid name. It provides deterministic `InvalidInput` errors before side
effects, ensures a single component is not interpreted as a path, and prevents
Windows device names such as `COM1` from being interpreted as devices rather
than ordinary files.

## LocalTempFile Close Contract

`LocalTempFile::close(&mut self) -> ()` remains infallible and only releases the
owned unbuffered `std::fs::File` handle. It does not claim to report operating
system close errors, flush a userspace buffer, or provide durability. Callers
that require durability use `as_file()?.sync_all()` before closing.

This contract matches the two `rs-mime` staging helpers, which close the handle
before passing the path to a path-based backend, especially for Windows file
sharing. The English and Chinese guides remove the inaccurate statement that
temporary-file persistence flushes the handle. No downstream migration is
required.

## Buffer Capacity Invariants

`FileBuffering::Buffered` stores a custom capacity as
`Option<NonZeroUsize>`. A zero-capacity policy can no longer be represented.
The public convenience APIs continue accepting `usize` so configuration values
do not require callers to construct a `NonZeroUsize` manually, but they become
fallible:

```rust
pub fn buffered_with_capacity(capacity: usize) -> io::Result<Self>;
```

The corresponding `FileBuffering`, `FileReadOptions`, and
`FileWriteOptions` constructors or builders all return
`ErrorKind::InvalidInput` for zero. Validation therefore occurs while building
options, before either reader or writer performs metadata inspection, creates
parents, opens a file, truncates a target, or creates a new entry. Reader and
writer construction consume only valid policies and no longer contain
duplicated late validation.

## Must-use Builders

`FileBuffering`, `FileReadOptions`, and `FileWriteOptions` receive type-level
`#[must_use]` annotations. Every builder that consumes or creates an options
value and returns an updated value receives a message-bearing `#[must_use]`
annotation where the type-level annotation is not sufficiently descriptive.

A compile-fail Rustdoc test enables `deny(unused_must_use)` and intentionally
ignores a builder result. It compiles before the annotation and therefore
fails the compile-fail test, then fails compilation as intended after the
annotation is added.

## Internal Module Layout

`local_files.rs` remains the public facade containing the `LocalFiles`
unconstructible marker type, its public associated methods, and their Rustdoc.
Its private implementation is moved under a dedicated lower-level `internal`
module so internal machinery is visually separated from the public types that
use it:

```text
src/local/
  local_files.rs
  local_temp_file.rs
  local_temp_dir.rs
  ... public-type modules ...
  internal/
    mod.rs
    path_io_error.rs
    path_operations.rs
    file_io.rs
    temp_entry.rs
    file_move.rs
    staged_file.rs
    atomic_write.rs
    copy_dir.rs
```

Responsibilities are:

- `path_io_error.rs`: the private path-context error type and its trait
  implementations;
- `path_operations.rs`: existence, metadata, directory listing, parent and
  directory creation, size calculation, cleaning, generic removal, path error
  context, and shared parent-path helpers;
- `file_io.rs`: reader and writer opening plus application of read/write
  options;
- `temp_entry.rs`: private temporary file and directory creation, retry
  validation, and private directory modes;
- `file_move.rs`: replace/no-replace moves, parent-directory synchronization,
  path conversion, and Linux, macOS, and Windows FFI;
- `staged_file.rs`: panic-safe ownership and cleanup of uncommitted staging
  files;
- `atomic_write.rs`: atomic-write staging, permission preservation, commit, and
  stage-aware errors;
- `copy_dir.rs`: recursive traversal, symlink policy, conflict handling,
  staging, commit, partial statistics, and cycle/destination checks.

`internal/mod.rs` only declares internal modules and re-exports the narrowly
needed `pub(crate)` entry points. Each concrete source file imports its direct
dependencies explicitly; child modules do not inherit a shared prelude from
`internal/mod.rs`.

The public facade delegates to these modules. `LocalTempFile` and
`LocalTempDir` call narrowly scoped internal functions instead of importing a
mixed collection of helpers from `local_files.rs`. The split is performed only
after behavioral tests are green and does not change public behavior.

## Recursive Copy Documentation

When a regular source file conflicts with an existing destination directory
and `LocalCopyTypeConflictPolicy::Replace` is selected, the source contents are
fully staged before the destination directory is removed. A source-open or
copy failure therefore leaves the existing destination intact. The final
remove-and-move sequence cannot be made fully atomic across unlike entry types;
an explanatory implementation comment and public documentation state the
remaining failure window.

Rustdoc, the English and Chinese READMEs, and both user guides explicitly state
that recursive copy is not a tree-level transaction. An error may leave
created directories, committed files, overwritten targets, removed conflicting
entries, and partial statistics. No rollback is attempted.

The documentation also calls out that
`LocalCopyTypeConflictPolicy::Replace` may recursively remove an existing
destination directory before a later operation fails. This destructive behavior
is reachable only through explicit policy selection.

## Persistence Documentation

Temporary resource persistence uses native rename/move semantics and does not
fall back to copy-then-delete. Moving across filesystems may therefore fail
with `EXDEV` on Unix or a platform-equivalent error.

When file persistence overwrites an existing target, the resulting entry keeps
the temporary file's permissions, which are normally private on Unix; it does
not preserve the replaced target's permissions. This is distinguished from
`LocalFiles::atomic_write`, which preserves existing regular-file permissions
before replacement.

These constraints are documented on `LocalTempFile::persist`,
`LocalTempFile::persist_with`, and directory persistence where applicable, and
are repeated in the English and Chinese guides.

## API Hygiene and Style Debt

- `FileBuffering::Buffered` stores `Option<NonZeroUsize>` and all custom
  capacity builders reject zero before filesystem I/O.
- `FileBuffering`, `FileReadOptions`, and `FileWriteOptions` plus their
  value-returning builders are `#[must_use]`.
- The empty public namespace enums become unconstructible public marker
  structs, preserving associated-function call paths without using an enum
  that represents no state.
- The unused duplicate `LocalFiles::DEFAULT_TEMP_FILE_PREFIX` is removed;
  `LocalFilenames::DEFAULT_RANDOM_PREFIX` remains the canonical constant.
- Every source module has module Rustdoc, private helper types live one per
  file under `internal`, and externally observable tests are organized under
  mirrored `tests/local/*_tests.rs` modules.
- Inline attributes follow the repository decision table: pure forwarding,
  getters, and setters use `#[inline(always)]`; short non-forwarding helpers
  may use `#[inline]`; complex control flow has no inline attribute.

The opt-in macOS CI work from the earlier plan is already present and is not
reimplemented by this correction.

## TDD and Verification

Implementation proceeds in this order:

1. reproduce and fix the dangling final-symlink escape;
2. add Windows regressions and fix NUL conversion and directory-symlink
   removal;
3. reproduce callback-panic staging leakage and make staging cleanup RAII;
4. reproduce destructive file-to-directory replacement ordering and stage
   before removal;
5. reproduce and fix superscript Windows device names;
6. reproduce zero-capacity construction and replace invalid states with
   `NonZeroUsize`, then enforce must-use builders;
7. split `local_files.rs` into `local::internal` while behavior remains green;
8. finish marker-type, constant, Rustdoc, test-layout, method-order, and inline
   hygiene;
9. document recursive-copy, persistence, callback-panic, path-length, and
   close constraints in English and Chinese.

Final local gates follow the repository correction sequence: `./align-ci.sh`,
then `./ci-check.sh`, and `./coverage.sh json` only if CI reports coverage below
its threshold. The affected `rs-mime` suite runs against the local 0.3 crate.
Windows and macOS runtime behavior remains finally exercised by the configured
GitHub Actions jobs.
