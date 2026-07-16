# rs-local-files Strong Invariants Design

## Goal

Strengthen `qubit-local-files` around ordinary-file opening, atomic callback
ownership, temporary-resource paths, arithmetic overflow, and unsupported
platform behavior. The implementation deliberately accepts source-breaking
and behavior-breaking changes so each public contract has one authoritative
state and unsafe capabilities cannot escape through convenience APIs.

## Scope and constraints

- Rust 2024 and Rust 1.94 remain the minimum language and toolchain targets.
- The crate remains synchronous and does not add an async runtime.
- `LocalFiles::open_reader` and `LocalFiles::open_writer` become ordinary-file
  APIs; directories, FIFOs, sockets, devices, and other special files are
  rejected.
- `LocalFiles::atomic_write_with` no longer exposes `std::fs::File`.
- `LocalAtomicWriter` remains a streaming `Write` implementation without
  `Seek`, raw-handle access, or access to its underlying `File`.
- `LocalTempFile` and `LocalTempDir` expose only stable absolute paths.
- Tests stay under `tests/`; production visibility is not widened solely for
  tests.
- Behavioral fixes use RED-GREEN regression tests before production edits
  where the failure can be reproduced safely and portably.
- No package-version bump, downstream manifest edit, git commit, or push is
  part of this implementation. Release coordination remains a separate task.
- The attacker-resistant rooted filesystem described in the companion
  `LocalRoot` design is not implemented in this change.

## Ordinary-file opening

### Public contract

`LocalFiles::open_reader` and `LocalFiles::open_writer` return handles only for
ordinary files. Existing non-file entries are rejected with
`io::ErrorKind::InvalidInput`. A writer mode that permits creation may still
create a missing ordinary file.

Symbolic links retain their current behavior outside rooted APIs: a link to an
ordinary file may be opened. This API is path-based and does not claim root
containment. The future `LocalRoot` API will provide the stronger no-symlink
contract required by a sandbox.

### Open sequence

The private `file_io` implementation uses three layers:

1. Inspect an existing path before opening it. This rejects an already-present
   FIFO, socket, device, or directory before the operating system can block or
   trigger device-specific behavior.
2. On Unix, add `O_NONBLOCK` to the actual open. This closes the check/open race
   in which another actor replaces a previously checked ordinary file with a
   FIFO. After the returned handle is verified as an ordinary file, clear
   `O_NONBLOCK` before exposing the handle to the caller.
3. Inspect metadata through the opened handle. This final check rejects a
   special file that replaced the path between the preflight inspection and
   open.

Both the preflight and non-blocking open remain necessary: preflight avoids
unwanted special-file opens in the normal case, while `O_NONBLOCK` prevents a
concurrent FIFO replacement from hanging the process. Source comments at the
open helper explain this division explicitly.

Using `O_NONBLOCK` and clearing it with `fcntl` requires `libc` on Unix, so the
existing Linux-only dependency becomes a Unix-target dependency. Windows uses
the preflight and handle checks; this change does not claim attacker-resistant
Windows named-pipe handling.

`open_writer` validates all non-file targets before a create-or-truncate or
append open whenever the target already exists. The handle check remains after
open because the path can change concurrently.

### Regression tests

Unix integration tests create a FIFO with `libc::mkfifo` and invoke reader and
writer opening on separate threads. Each test waits on a channel with a bounded
timeout. If the old implementation blocks, the test opens the complementary
FIFO end to release and join the worker before failing; the test suite itself
therefore cannot retain a permanently blocked thread. The passing behavior is
an immediate `InvalidInput` error.

Existing directory and regular-file tests remain and are updated to assert the
unified ordinary-file contract.

## Guarded atomic callback

The public callback changes to:

```rust
pub fn atomic_write_with<P, F>(
    path: P,
    write: F,
) -> Result<(), LocalAtomicWriteError>
where
    P: AsRef<Path>,
    F: FnOnce(&mut LocalAtomicWriter) -> io::Result<()>;
```

`LocalAtomicWriter::write_with` invokes the callback with `&mut self`. On
success it commits the same owned staging object. On callback error it maps the
error to `LocalAtomicWriteStage::WriteTemporaryFile`, explicitly attempts
staging cleanup, and preserves a secondary cleanup failure. A panic unwinds
through the armed staging guard, retaining the current best-effort cleanup
behavior.

No `File` clone is created. The callback cannot call `try_clone`, replace the
canonical handle, or retain a handle that can mutate the destination after
rename. The implementation carries a source comment explaining that exposing
`File` would allow a cloned handle to outlive commit and invalidate the atomic
snapshot guarantee.

The initial guarded callback deliberately supports only `Write`. There is no
in-tree caller requiring `File`-specific methods or `Seek`, and adding those
capabilities would weaken the smallest useful contract.

Tests include:

- a compile-fail doctest proving `try_clone` is unavailable;
- a callback explicitly typed as `&mut LocalAtomicWriter` that writes and
  commits data;
- existing callback-error, cleanup-error, and panic-cleanup cases;
- removal of the obsolete test that replaces the callback's `File` value.

## Single-source temporary paths

`LocalTempFile` stores only:

```rust
path: Option<PathBuf>,
file: Option<File>,
```

`LocalTempDir` stores only:

```rust
path: Option<PathBuf>,
```

Creation resolves the requested parent with the existing `absolute_path`
helper before generating an entry. The resulting absolute generated path is
the sole path used for display, metadata, child operations, cleanup,
persistence, `Drop`, and ownership release.

The observable contract becomes:

- `path()` always returns an absolute path;
- `child_path()` and ensured child paths are absolute;
- `keep()` returns the absolute generated path;
- successful `persist()` and `persist_with()` return the absolute final path,
  including when the caller supplied a relative target;
- a persistence error retains a guard whose `path()` is the same absolute
  source used by the failed operation.

The duplicate `operation_path` fields and private accessors are removed. This
eliminates the possibility that caller-facing and operational state diverge
after a current-directory change.

Regression tests create resources from relative parents, change the process
current directory under the existing global lock, and verify that `path`,
`keep`, child helpers, and persistence results remain directly usable without
joining them to the creation directory.

The English and Chinese README and user guide describe the new absolute-path
contract. `rs-mime` requires no source migration because it treats temporary
paths as opaque local paths.

## Unsupported platform fallback

`file_move.rs` adds a
`#[cfg(not(any(unix, windows)))] move_file_without_replacing` implementation.
It always returns `io::ErrorKind::Unsupported` and includes both source and
destination paths in the message. Supported Unix targets retain the native or
hard-link implementation, and Windows retains its native move implementation.

A target-specific integration test is compiled only on non-Unix/non-Windows
targets and asserts `Unsupported` through temporary-file persistence. On the
development host, verification attempts `cargo check --target wasm32-wasip1`
when that standard-library target is installed; lack of a local runner does not
justify emulating the cfg branch on a supported host.

## Checked directory-size arithmetic

`dir_size_recursive` computes each child contribution, then combines it with
the accumulated total using `u64::checked_add`. Overflow returns
`io::ErrorKind::InvalidData` with the child path whose contribution exceeded
the representable total. Debug and release builds therefore share the same
error behavior.

A portable end-to-end overflow fixture is not added: common filesystems cannot
reliably create enough representable sparse-file length to overflow `u64`, and
introducing an injectable filesystem abstraction or widening a private helper
solely for a test would be more harmful than the uncovered two-line branch.
Existing normal, recursive, symlink, missing-path, and permission-error
directory-size tests continue to exercise the surrounding calculation.

## Documentation and style corrections

The same change set performs only the approved focused cleanup:

- describe portable filename validation as rejecting Unicode control
  characters, matching `char::is_control`, and test one C1 control character;
- move `begin_atomic_write` adjacent to `atomic_write` and
  `atomic_write_with`;
- change the identified pure forwarding methods to `#[inline(always)]` and
  the branch-bearing filename helper to `#[inline]`;
- replace incidental `unwrap` calls only in tests touched by this work;
- update public Rustdoc, README files, and user guides for the ordinary-file,
  guarded-callback, and absolute temporary-path contracts.

No unrelated source split, public-field cleanup, or bulk test rewrite is part
of this design.

## Verification

Each reproducible behavior change runs its focused test in RED state before
the production edit and in GREEN state afterward. Final validation runs:

1. focused `local_tests` modules for file I/O, atomic writes, temporary files,
   temporary directories, path operations, and filenames;
2. `cargo +1.94.0 test --doc`;
3. `cargo +1.94.0 check --all-targets`;
4. `cargo +1.94.0 check --target wasm32-wasip1` when installed;
5. `./align-ci.sh`;
6. `./ci-check.sh`;
7. `./coverage.sh json` when the CI result requires a coverage check;
8. `cargo +1.94.0 test --manifest-path ../rs-mime/Cargo.toml` to verify the
   direct downstream against the local path dependency.

Platform-specific behavior that cannot run locally remains assigned to the
configured macOS and Windows CI jobs and is reported explicitly.
