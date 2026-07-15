# rs-local-files 0.3 Hardening Design

## Goal

Harden `qubit-local-files` so temporary resources are private, destructive
operations validate before mutating, copy and persistence operations have
race-resistant conflict semantics, and multi-stage failures retain enough
state for callers to recover or diagnose partial effects.

This is a source-breaking 0.3 release. The old public APIs do not need
compatibility shims.

## Crate Boundary

The crate remains a synchronous, concrete local-filesystem utility. It does
not depend on `qubit-fs`, add asynchronous APIs, or become a security sandbox.
Policy names should align with `qubit-fs` where useful, but local operations
continue to use `std::path::Path` and native filesystem errors.

`LocalTempDir` child helpers reject lexical traversal and symlinks observed
during validation. They are explicitly not a security boundary against a
concurrent process that can mutate the same directory tree. Descriptor-relative
or capability-based traversal is outside this release.

## Temporary Resources

On Unix, temporary files are created with mode `0o600` and temporary
directories with mode `0o700`. Child directories created inside a
`LocalTempDir` use the same private directory mode. Windows keeps inherited ACL
semantics.

`LocalTempFile` owns its original read/write `File` handle. It implements
`Write` and `Seek` directly and exposes borrowed file-handle access. The
`writer(FileWriteOptions)` state machine is removed. Callers that need
buffering wrap `&mut LocalTempFile` in `BufWriter`.

`close(&mut self)` flushes the live handle and only drops it after flushing
succeeds. A failed close therefore leaves the handle available and cannot be
mistaken for a successful close.

Persistence failures return `LocalPersistError<T>`, which owns both the native
`io::Error` and the temporary resource. Callers can inspect the error, retry,
keep, or clean up the recovered guard. Directory persistence uses native
no-replace moves on Linux, macOS, and Windows; targets supported only by a
check-then-rename fallback return `Unsupported` instead of weakening the
no-replace contract.

## Write Validation

All `FileWriteOptions` validation runs before parent creation, file creation,
truncation, or append opening. A zero buffering capacity remains a runtime
`InvalidInput` error, but it cannot mutate the target or create parents.

## Recursive Copy

`LocalCopyDirOptions::overwrite` is replaced by:

- `conflict: LocalCopyConflictPolicy` with `Fail`, `Overwrite`, and `Skip`;
- `type_conflict: LocalCopyTypeConflictPolicy` with `Fail` and `Replace`.

Existing destination directories that correspond to source directories are
merged. File-entry conflicts use `conflict`. A file/directory type mismatch
uses `type_conflict`; recursive deletion occurs only when the caller explicitly
selects `Replace`.

Every regular file is copied to a private same-directory staging file. Commit
uses a native no-replace move for `Fail` and `Skip`, or atomic replacement for
`Overwrite`. This removes the metadata-check-then-`fs::copy` overwrite race.
`Skip` increments a new `LocalCopyDirStats::skipped` counter.

When `preserve_permissions` is false, newly created files remain `0o600` and
directories remain `0o700` on Unix. When true, portable source permissions are
applied before committing a file and after copying a directory's children.

## Structured Errors

`atomic_write` and `atomic_write_with` return `LocalAtomicWriteError`. The
error records the destination, optional staging path, `LocalAtomicWriteStage`,
whether replacement already committed, and the original `io::Error` as its
source.

`copy_dir_all_with` returns `LocalCopyDirError`. The error records
`LocalCopyDirStage`, the source and destination entries being processed,
partial `LocalCopyDirStats`, and the original `io::Error`.

Simple single-stage helpers continue returning `io::Result`. Their path-context
wrapper retains the original error in the error source chain instead of
flattening it into a string.

## Downstream Migration

`rs-mime` migrates temporary staging to `Write::write_all` and `io::copy`
directly against `LocalTempFile`, followed by `close()` before invoking
path-based native detectors. Its dependency requirement moves from 0.2 to 0.3.

## Verification

Each defect is reproduced by a regression test before its implementation is
changed. Required final gates are the project rustfmt configuration, all-target
tests, doctests, Clippy with warnings denied, rustdoc with warnings denied, and
the affected `rs-mime` tests against the local 0.3 crate.
