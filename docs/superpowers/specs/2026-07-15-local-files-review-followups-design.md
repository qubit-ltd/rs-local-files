# rs-local-files Review Follow-ups Design

## Goal

Resolve the correctness and API issues identified during the `rs-local-files`
0.3 review, split the oversized `local_files.rs` implementation into focused
private modules, document non-transactional and platform-dependent behavior,
and add opt-in macOS CI coverage for the crate's macOS-specific filesystem
code.

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

## LocalTempFile Close State

`LocalTempFile` continues to store its live handle as `Option<File>`. It gains:

```rust
pub fn close(&mut self) -> io::Result<()>;
pub const fn is_closed(&self) -> bool;
```

The first `close` call flushes the live file. The handle is removed and dropped
only after flushing succeeds. A flush failure leaves the handle open so the
caller can inspect or retry it. Calling `close` after the handle has already
been closed returns the existing closed-handle `ErrorKind::NotFound` error.

Repeated `flush` calls while the file is open remain valid. `flush`, `write`,
and `seek` after close return the closed-handle error. `flush` only transfers
userspace buffered data to the operating system and is not a durability
guarantee; callers that require durability must use `File::sync_all` through
the exposed file handle.

Operations that close implicitly propagate the new failure path:

- `cleanup` returns a close or removal error;
- `keep` returns `io::Result<PathBuf>`;
- `persist` and `persist_with` return `LocalPersistError` retaining the guard
  when close or move fails;
- `Drop` logs a close failure, forcibly drops the handle because it cannot
  return an error, and continues best-effort path cleanup.

`LocalFileWriter::close(self)` is unchanged. It consumes the writer, so a
second close is prevented by ownership instead of represented as runtime
state.

The two current `rs-mime` staging helpers migrate from `file.close()` to
`file.close()?` and are tested against the local 0.3 crate.

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
namespace, its public constants, public associated methods, and their Rustdoc.
Its private implementation is moved under a dedicated lower-level `inner`
module so internal machinery is visually separated from the public types that
use it:

```text
src/local/
  local_files.rs
  local_temp_file.rs
  local_temp_dir.rs
  ... public-type modules ...
  inner/
    mod.rs
    path_operations.rs
    file_io.rs
    temp_entry.rs
    file_move.rs
    atomic_write.rs
    copy_dir.rs
```

Responsibilities are:

- `path_operations.rs`: existence, metadata, directory listing, parent and
  directory creation, size calculation, cleaning, generic removal, path error
  context, and shared parent-path helpers;
- `file_io.rs`: reader and writer opening plus application of read/write
  options;
- `temp_entry.rs`: private temporary file and directory creation, retry
  validation, and private directory modes;
- `file_move.rs`: replace/no-replace moves, parent-directory synchronization,
  path conversion, and Linux, macOS, and Windows FFI;
- `atomic_write.rs`: atomic-write staging, permission preservation, commit, and
  stage-aware errors;
- `copy_dir.rs`: recursive traversal, symlink policy, conflict handling,
  staging, commit, partial statistics, and cycle/destination checks.

`inner/mod.rs` only declares internal modules and re-exports the narrowly
needed `pub(crate)` entry points. Each concrete source file imports its direct
dependencies explicitly; child modules do not inherit a shared prelude from
`inner/mod.rs`.

The public facade delegates to these modules. `LocalTempFile` and
`LocalTempDir` call narrowly scoped internal functions instead of importing a
mixed collection of helpers from `local_files.rs`. The split is performed only
after behavioral tests are green and does not change public behavior.

## Recursive Copy Documentation

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

## Opt-in macOS CI

The reusable `rs-ci` workflow gains this input:

```yaml
run_macos_tests:
  description: Run clippy and tests on the pinned macOS runner.
  required: false
  type: boolean
  default: false
```

The new `macos_test` job runs only when the input is true and the triggering
event is not a schedule. It uses `macos-15`, depends on `fast_checks`, checks
out recursive submodules, restores a runner-specific Cargo cache, installs the
configured build toolchain and Clippy, runs Clippy for all targets and features
with warnings denied, and runs all-feature tests verbosely.

Existing `rs-ci` consumers remain unaffected. The `rs-local-files` caller sets
`run_macos_tests: true` so its macOS-specific `renamex_np` path is compiled and
exercised. The `rs-ci` English and Chinese READMEs document the new opt-in
input.

After local workflow validation and the existing `rs-ci` test suite pass, the
`rs-ci` change is committed with an English Angular-style message, merged into
`dev` and `main`, and pushed. The working branch is returned to
`dev-starfish` and pushed. Any fetch, merge, or push conflict stops the process
for user direction. Finally, `rs-local-files` runs the repository's actual
`./update-submodule.sh` script to update `.rs-ci` from its configured `main`
tracking branch.

## TDD and Verification

Implementation proceeds in this order:

1. reproduce and fix the dangling final-symlink escape;
2. reproduce and fix superscript Windows device names;
3. reproduce and fix close flushing and state transitions, then migrate
   `rs-mime`;
4. reproduce zero-capacity construction and replace invalid states with
   `NonZeroUsize`;
5. add and satisfy the must-use compile-fail Rustdoc;
6. split `local_files.rs` while all behavior tests remain green;
7. document recursive-copy and persistence constraints;
8. add and locally validate the opt-in macOS workflow;
9. publish `rs-ci` as authorized and update the `rs-local-files` submodule.

Final local gates for `rs-local-files` are the repository-pinned rustfmt check,
style check, all-target/all-feature Clippy with warnings denied, all-feature
tests, doctests, and Rustdoc with warnings denied. The affected `rs-mime` tests
run against the local 0.3 crate. `rs-ci` runs its Python, Node, shell/style, and
workflow syntax checks. Actual macOS execution occurs through the enabled
GitHub Actions job when the `rs-local-files` caller runs remotely.
