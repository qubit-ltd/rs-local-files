# LocalRoot Capability Design

## Goal

Define the support `qubit-local-files` must eventually provide so a planned
`qubit-fs-local` adapter can enforce an attacker-resistant filesystem root.
This document defines a separate capability API; it does not change the
path-based `LocalFiles` contract and is not implemented by the current strong
invariants change.

## Security boundary

A canonical `PathBuf`, lexical normalization, and `starts_with(root)` checks
are not a sandbox. An attacker that can modify the filesystem namespace can
replace a checked component with a symbolic link or reparse point before a
later open, rename, or removal.

`LocalRoot` therefore owns an open directory capability. Every descendant
lookup and mutation must remain relative to that capability through the final
filesystem operation. The stored absolute root path exists only for display
and diagnostics; it never authorizes access.

If a target cannot implement a requested operation without falling back to a
check-then-path sequence, that operation returns `io::ErrorKind::Unsupported`.
The implementation must not silently weaken the containment contract.

## Public types

The first rooted release adds one type per source file:

```rust
pub struct LocalRelativePath { /* validated owned components */ }

impl LocalRelativePath {
    pub fn new<P: AsRef<Path>>(path: P) -> io::Result<Self>;
    pub fn as_path(&self) -> &Path;
}

pub struct LocalRoot { /* open root capability plus diagnostic path */ }

impl LocalRoot {
    pub fn open<P: AsRef<Path>>(root: P) -> io::Result<Self>;
    pub fn path(&self) -> &Path;
    pub fn open_reader(
        &self,
        path: &LocalRelativePath,
        options: FileReadOptions,
    ) -> io::Result<LocalFileReader>;
    pub fn open_writer(
        &self,
        path: &LocalRelativePath,
        options: FileWriteOptions,
    ) -> io::Result<LocalFileWriter>;
    pub fn begin_atomic_write(
        &self,
        path: &LocalRelativePath,
    ) -> Result<LocalRootAtomicWriter, LocalAtomicWriteError>;
}

pub struct LocalRootAtomicWriter { /* root-relative staging lifecycle */ }

impl LocalRootAtomicWriter {
    pub fn commit(self) -> Result<(), LocalAtomicWriteError>;
    pub fn abort(self) -> Result<(), LocalAtomicWriteError>;
}
```

`LocalRootAtomicWriter` implements `Write`, but does not expose its underlying
file or directory handles. It is separate from `LocalAtomicWriter` because the
rooted writer commits by parent-directory capability and entry name, while the
existing writer commits by ordinary filesystem path. Hiding both backends
inside one public struct would obscure the security boundary.

## Relative path contract

`LocalRelativePath::new` accepts a non-empty sequence of normal path
components. It rejects:

- absolute paths;
- platform prefixes and root components;
- `.` and `..` components;
- empty paths;
- embedded NUL values rejected by the target platform.

Validation is lexical and strong typing prevents rooted methods from
accidentally accepting an unchecked `Path`. It is not, by itself, the sandbox;
the owned root capability and descriptor-relative operations provide that
guarantee.

## Symlink and reparse-point policy

The first version has one policy: deny symbolic links and name-surrogate
reparse points at every traversed component and at the final entry. No public
policy enum is added until a concrete requirement for following links exists.

Following a link “only when it remains inside the root” requires additional
handle-based resolution semantics and is intentionally absent. This keeps the
initial contract auditable and maps cleanly to a secure default for
`qubit-fs-local`.

## Descriptor-relative operations

Each rooted operation performs component traversal from the open root handle:

1. Open each intermediate component as a directory without following links.
2. Verify the opened object through its handle.
3. Carry the parent directory handle into the final operation.
4. Open, create, rename, or remove the final entry relative to that parent
   handle without following a final link.

Unix implementations use `openat`-family directory descriptors, no-follow
flags, handle metadata, and `renameat`/native no-replace operations. Linux may
use `openat2` resolution flags as an optimization and stronger single-syscall
path, but correctness cannot depend on kernel support for `openat2`.

Windows support must open and inspect directory/file handles while rejecting
name-surrogate reparse points. If the chosen Windows primitives cannot make a
specific mutation relative to an anchored root without a path race, that
operation is `Unsupported` until a safe backend exists.

The implementation phase must choose between a mature capability filesystem
dependency and direct platform backends by proving that the selected approach
supports the exact open, atomic rename, directory synchronization, and
no-replace operations above. Dependency convenience alone is not sufficient.

## Atomic write semantics

Rooted atomic replacement creates the staging file relative to the destination
parent directory capability, not by constructing an absolute staging path.
Commit then:

1. applies preserved permissions when the existing final entry is an ordinary
   file;
2. synchronizes the staging file;
3. closes the staging data handle;
4. renames the staging entry over the destination relative to the same parent
   directory capability;
5. synchronizes that parent directory capability;
6. disarms cleanup only after rename succeeds.

Errors retain the requested `LocalRelativePath`, an optional diagnostic
staging name, operation stage, commit state, primary source error, and cleanup
error. Diagnostic paths must not be reused internally as authority.

## Delivery phases

The rooted subsystem is implemented separately in three reviewable phases:

1. `LocalRelativePath`, `LocalRoot::open`, rooted ordinary-file reader/writer,
   symlink-denial tests, and race-focused platform tests.
2. `LocalRootAtomicWriter`, descriptor-relative staging, no-handle callback,
   commit/abort cleanup, and parent-directory durability tests.
3. Rooted temporary resources and recursive copy only after `qubit-fs-local`
   demonstrates a concrete need. Their traversal and commit logic must reuse
   the rooted primitives rather than wrap current path-based helpers.

This order gives `qubit-fs-local` the minimum secure read/write/atomic surface
without prematurely duplicating the entire `LocalFiles` namespace.

## Testing requirements

Every rooted method requires external integration tests for:

- rejection of absolute, parent, root, prefix, empty, and NUL paths;
- symlinks or reparse points at every component position;
- final-entry replacement attempts;
- concurrent component replacement coordinated with test barriers;
- root rename after `LocalRoot::open`, proving the handle rather than the
  diagnostic path is authoritative;
- ordinary success and structured error propagation;
- platform `Unsupported` behavior where a secure primitive is unavailable.

Security regressions must coordinate races deterministically through barriers
or process boundaries. Timing-only tests are not accepted as evidence of
containment.

## Downstream integration

`qubit-fs-local` owns URI/FsPath mapping, provider configuration, metadata
translation, and policy selection. After mapping an `FsPath` to a validated
`LocalRelativePath`, it delegates local reads, writes, and atomic replacement
to `LocalRoot`.

`qubit-fs` itself does not depend on `qubit-local-files`. The dependency
direction remains:

```text
qubit-fs-local -> qubit-fs
qubit-fs-local -> qubit-local-files
```

Existing `LocalFiles`, `LocalTempFile`, and `LocalTempDir` remain convenient
path-based tools for trusted local application code and continue to document
that they are not attacker-resistant sandbox boundaries.
