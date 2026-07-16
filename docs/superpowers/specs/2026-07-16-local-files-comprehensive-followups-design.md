# rs-local-files Comprehensive Follow-ups Design

## Goal

Implement the seven follow-ups approved after re-evaluating
`rs-local-files` against every in-tree `rs-*` consumer. Preserve the crate's
synchronous, standard-library-first boundary while correcting atomic-write
invariants, tightening configuration APIs, documenting filesystem race
boundaries, and adding the streaming atomic lifecycle required by the planned
`qubit-fs-local` adapter.

## Scope and constraints

- Rust 2024 and Rust 1.94 remain the minimum language and toolchain targets.
- No async runtime, virtual-filesystem abstraction, or new runtime dependency
  is introduced.
- Existing `LocalFiles::atomic_write` and `atomic_write_with` call shapes remain
  available.
- The new streaming atomic writer implements `Write`, but not `Seek`.
- Configuration-field privacy is an explicitly approved source-breaking
  change. Statistics and structured-error fields remain public.
- The source-breaking release increments `qubit-local-files` from `0.4.0` to
  `0.5.0`; the in-tree `rs-mime` dependency requirement and lockfiles advance
  with it.
- Behavioral changes use RED-GREEN regression tests before production edits.
- Tests remain external under `tests/`; production visibility is not widened
  solely for tests.

## Atomic destination permissions

Existing permissions are preserved only when the destination directory entry
itself is a regular file. Inspection uses `symlink_metadata`, so a symbolic
link to a regular file does not donate its target's permissions to the new
ordinary file that replaces the link. On Unix, such a replacement retains the
private staging mode `0o600`, subject to a more restrictive process umask.

The regression creates a permissive regular-file target and a symbolic link to
it, atomically writes through the link path, and verifies that the target is
unchanged while the replacement does not inherit the target's permissive mode.

## Atomic callback handle invariant

The staging guard always retains a canonical handle for the filesystem object
whose path will be committed. `atomic_write_with` passes a cloned handle to the
callback. Replacing the callback's `File` value therefore cannot redirect
permission preservation or synchronization away from the staging object.

The internal callback contract becomes generic `FnOnce`; the public wrapper no
longer adapts it through `Option`, `FnMut`, dynamic dispatch, or an impossible
second-call panic. A Linux regression replaces the callback handle with
`/dev/full` after writing and verifies that the original staging object is
still synchronized and committed successfully.

## Existing-prefix canonicalization

`canonicalize_existing_prefix` uses `Path::try_exists` for both its initial
check and ancestor walk. Inspection errors are returned immediately instead
of being reclassified as missing components. A Unix regression combines a
missing source with an interior-NUL destination and verifies that destination
validation fails first at `PrepareDestination`.

## Recursive-copy concurrency contract

The public Rustdoc, README files, and user guides explicitly state that source
metadata inspection, source opening, destination reinspection, and destructive
replacement are separate path-based operations. Consequently:

- `follow_symlinks = false` prevents ordinary accidental traversal but is not
  a sandbox guarantee against concurrent path replacement;
- type-conflict replacement can act on an entry changed by a concurrent actor;
- callers requiring an attacker-resistant root must use a future
  descriptor/capability-relative local-filesystem adapter.

No additional path-level precheck is added because it cannot close the race.

## Streaming atomic writer

Add a public `LocalAtomicWriter` in its own source file and export it from the
crate root. Construction remains namespaced:

```rust
impl LocalFiles {
    pub fn begin_atomic_write<P>(
        path: P,
    ) -> Result<LocalAtomicWriter, LocalAtomicWriteError>
    where
        P: AsRef<Path>;
}
```

`LocalAtomicWriter` owns the destination path, created-parent synchronization
state, existing destination permissions, and an armed `StagedFile`. It
implements `std::io::Write` by delegating to the canonical staging handle and
provides:

```rust
pub fn commit(self) -> Result<(), LocalAtomicWriteError>;
pub fn abort(self) -> Result<(), LocalAtomicWriteError>;
```

`commit` applies preserved permissions, synchronizes the staging file, closes
it, replaces the destination, disarms cleanup, and synchronizes the destination
parent chain. Pre-commit errors explicitly clean the staging file and retain a
secondary cleanup error. Post-replacement parent-sync errors report
`committed = true` exactly as today.

`abort` closes and removes the staging path. A new non-exhaustive
`LocalAtomicWriteStage::CleanupTemporaryFile` variant identifies explicit
abort failures. Dropping an uncommitted writer delegates to `StagedFile`'s
best-effort cleanup and warning behavior.

`atomic_write` begins a writer, writes all bytes, maps a write error to
`WriteTemporaryFile`, and commits. `atomic_write_with` begins a writer, invokes
the callback with a cloned `File`, maps callback or clone failures to
`WriteTemporaryFile`, and commits. This makes the existing APIs thin wrappers
over the same lifecycle used by future `rs-fs-local` `FileWriter` adapters.

The public tests cover commit visibility, explicit abort, implicit drop cleanup,
`Send`, wrapper compatibility, permission preservation, and structured error
state. The first version deliberately has no configurable durability switches,
temporary prefix, `Seek`, or public access to the underlying canonical handle.

## Configuration encapsulation

Make these configuration fields private while preserving their current
getters, constructors, builders, defaults, and derives:

- `FileReadOptions::buffering`;
- `FileWriteOptions::{create_parent, mode, buffering}`;
- `LocalCopyDirOptions::{conflict, type_conflict, follow_symlinks,
  preserve_permissions}`;
- `LocalPersistOptions::overwrite`.

Internal consumers switch to getters. Compile-fail doctests demonstrate that
direct field mutation is no longer supported and force external callers onto
the documented builder API. Policy enums stay exhaustive because no concrete
new variant is currently required.

## Rust style corrections

- Move factory and constructor methods before getters within each inherent
  implementation, across all visibility levels.
- Add `#[inline]` to the short `LocalFileWriter::sync_all` and `sync_data`
  dispatch methods; use `#[inline(always)]` only for pure forwarding methods.
- Move `FileAttributeTagInfo` and `FileDispositionInfo` into individual
  Windows-gated files under `src/local/internal`.
- Add item-level Rustdoc to module-level platform constants and foreign
  function declarations, and to the two undocumented module-level filename
  constants. Existing call-site `SAFETY` comments remain.
- Keep the repository-standard `# Parameters` heading rather than introducing
  a competing documentation convention.

These edits do not change runtime behavior and are performed only after the
behavioral suite is green.

## Documentation and downstream impact

English and Chinese README/user-guide text is updated for the new writer,
private option fields, symlink permission behavior, recursive-copy race
boundary, and the `0.5.0` version. `rs-mime` requires only its dependency
version and lockfile update because its production code uses
`FileReadOptions::buffered`, `LocalTempFile`, and `LocalFilenames` rather than
direct option-field mutation. `rs-magika` remains only a transitive consumer;
its lockfile advances after `rs-mime` consumes the new local-files release.

The planned `rs-fs-local` adapter can wrap `LocalAtomicWriter` directly for
`WriteMode::ReplaceAtomic` and map its `commit` and `abort` methods onto the
provider-neutral `FileWriter` lifecycle without buffering the whole resource
or duplicating the durability protocol.

## Verification

Each behavioral task runs its focused RED test before implementation and its
focused GREEN test afterward. Final validation runs, in order:

1. `./align-ci.sh`
2. `./style-check.sh`
3. `./ci-check.sh`
4. `./coverage.sh json`
5. `cargo +1.94.0 test --manifest-path ../rs-mime/Cargo.toml`

Platform-specific behavior is exercised on the host where possible and left
to the configured Windows and macOS CI jobs where the local host cannot run it.
