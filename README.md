# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Local filesystem utilities for Rust.

## Overview

Qubit Local Files contains the local filesystem utilities split out of
`qubit-io`. It is focused on concrete local paths and local filesystem entries:
temporary files and directories, filename helpers, recursive directory
operations, and durable same-directory atomic writes.

Use this crate when you need:

- RAII temporary files or directories that clean themselves up on drop;
- parent-directory creation before opening or writing local files;
- descriptor-anchored local roots for attacker-resistant relative file I/O;
- recursive directory cleanup, size calculation, or copy operations;
- conservative copy and persistence defaults that reject accidental overwrites;
- random, portable, and lexical filename helpers;
- durable replacement writes where readers should observe either the old complete
  file or the new complete file.

For detailed usage, examples, and API selection guidance, see the
[User Guide](doc/user_guide.md). API reference documentation is available on
[docs.rs](https://docs.rs/qubit-local-files).

For stream-level `std::io` traits, extension methods, wrappers, and codecs, see
[qubit-io](https://github.com/qubit-ltd/rs-io).

## Installation

```toml
[dependencies]
qubit-local-files = "0.7"
```

## Quick Example

```rust
use std::io::Write;

use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let work = LocalTempDir::with_prefix("qubit-local-files-readme-")?;
let src = work.path().join("src");
let dst = work.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("manifest.json"), br#"{"version":1}"#)?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

LocalFiles::atomic_write(dst.join("manifest.json"), br#"{"version":2}"#)?;

let final_path = work.path().join("result.txt");
std::fs::write(&final_path, "old payload")?;

let mut temp = LocalTempFile::with_affixes("qubit-local-files-", ".txt")?;
temp.write_all(b"new payload\n")?;
temp.persist_with(&final_path, LocalPersistOptions::new().with_overwrite())?;

assert_eq!("new payload\n", std::fs::read_to_string(&final_path)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

## Main Capabilities

### LocalFiles Namespace

`LocalFiles` groups small local filesystem operations that otherwise tend to
become repeated boilerplate:

| Method | Purpose |
| --- | --- |
| `exists` | Checks path existence with `std::io::Result<bool>` instead of silently swallowing errors. |
| `metadata` | Reads local path metadata. |
| `list` | Lists direct directory entries. |
| `open_reader` | Opens a regular file as `LocalFileReader` using `FileReadOptions`; rejects directories and special resources. |
| `open_writer` | Opens or creates a regular file as `LocalFileWriter` using `FileWriteOptions`; rejects directories and special resources. |
| `ensure_dir` | Creates a directory and missing ancestors. |
| `ensure_parent` | Creates missing parent directories for a file path. |
| `dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `clean_dir` | Removes all children from a directory while keeping the directory itself. |
| `remove_any` | Removes a file, directory tree, or symbolic link. |
| `copy_dir_all_with` | Recursively copies a local directory tree with explicit options and returns statistics. |
| `atomic_write` | Replaces a file through a durable same-directory temporary write. |
| `atomic_write_with` | Same as `atomic_write`, but passes a guarded `LocalAtomicWriter` to caller-provided write logic. |
| `begin_atomic_write` | Returns a streaming `LocalAtomicWriter` committed explicitly by the caller. |

### Temporary Files and Directories

`LocalTempFile` and `LocalTempDir` create real local filesystem entries and
remove them automatically on drop unless ownership is released with `keep` or
`persist`. Drop-time cleanup is best-effort; failures are reported through the
`log` facade with `warn!` and never panic.

`LocalTempFile` owns its original file handle and implements `Write` and `Seek`.
Use `as_file` / `as_file_mut` for direct handle access, and `close` to drop the
unbuffered handle before reusing the path from other APIs. `close` does not call
`sync_all`; explicitly synchronize the handle first when durability is needed.
The type intentionally does not provide read helpers; read the path through
`LocalFiles` or `std::fs` when that is needed.

`LocalTempDir::child_path` only validates and joins a non-empty relative path
made of normal lexical components; it does not inspect existing symbolic
links, and its result is not proof of filesystem containment. The
`ensure_child_dir`, `open_child_reader`, and `open_child_writer` helpers also
reject symbolic-link escapes observed during their filesystem checks.
`ensure_child_dir` creates nested parents like `mkdir -p`.

Filesystem validation is not atomic with later operations. These helpers are
not a sandbox boundary when an untrusted actor can mutate the tree concurrently.

`LocalTempFile::persist` rejects an existing target by default during the move
operation. Use `LocalTempFile::persist_with` and
`LocalPersistOptions::new().with_overwrite()` only when replacing an existing target
is intended. `LocalTempDir::persist` also rejects an existing target and does not
provide an overwrite option. A failed persistence operation returns
`LocalPersistError`, which retains the temporary guard for retry or inspection
and reports `ResolveTarget`, `PrepareParent`, or `InstallDestination` together
with the requested target and, once available, its resolved absolute path.
Persistence uses native move/rename operations without a copy-and-delete
fallback, so cross-filesystem moves may fail with `EXDEV` on Unix or an
equivalent platform error. Overwriting a file keeps the temporary file's
metadata rather than the replaced target's metadata; use
`LocalFiles::atomic_write` when strict platform-native metadata preservation is
required.

Native no-replace support is deliberately explicit:

| Operation | Linux | macOS | Windows | Other targets |
| --- | --- | --- | --- | --- |
| Temp file/dir default persist (no replace) | Supported | Supported | Supported | `Unsupported` |
| Recursive copy `Fail`/`Skip` file commit | Supported | Supported | Supported | `Unsupported` |
| Temp file overwrite persist | Supported | Supported | Supported | Uses ordinary replacement support |
| Recursive copy `Overwrite` | Supported | Supported | Supported | Uses ordinary replacement support |

On unsupported targets, failed persistence retains the temporary guard.
Recursive copy may create destination directories before a later file commit
returns `Unsupported`; it does not roll back the whole destination tree.

Relative temporary-resource creation directories and persistence targets are
bound to the process current directory when the resource or operation begins.
`path`, child-path helpers, `keep`, `persist`, and `persist_with` return absolute
paths that remain directly usable after later current-directory changes.
Relative atomic-write destinations are likewise bound when writing begins, so
later changes do not redirect commit or cleanup. On Windows, native moves do
not add a verbatim-path prefix, so native path-length and verbatim-path semantics
apply.
Relative source and destination paths for recursive copy are also bound when
copy begins, so later current-directory changes do not redirect traversal,
staging, or commit.
On Unix, temporary files are created with mode `0600` and temporary directories
with mode `0700` before applying the process umask.

### Read and Write Options

Normal file opening is intentionally explicit:

| Type | Purpose |
| --- | --- |
| `FileReadOptions` | Controls reader buffering. |
| `FileWriteOptions` | Controls parent creation, write mode, and writer buffering. |
| `FileBuffering` | Selects unbuffered I/O or buffered I/O with an optional capacity. |
| `FileWriteMode` | Selects `OpenExistingAtStart`, `CreateNew`, `CreateOrTruncate`, `AppendExisting`, or `AppendOrCreate`. |

Both open helpers return only regular files. Directories, FIFOs, sockets, and
other special filesystem resources are rejected; on Unix, FIFO rejection does
not wait for a peer.

`LocalFileReader` implements `Read` and `Seek`. `LocalFileWriter` implements
`Write` and `Seek`, and provides `sync_all` / `sync_data` helpers that flush any
buffered bytes before synchronizing the underlying file. Seeking a writer does
not disable append-mode semantics.

Custom-capacity constructors return `std::io::Result` and reject zero. The
stored custom capacity is a `NonZeroUsize`, so invalid buffering policies cannot
be passed to file-opening methods.

`atomic_write` remains a separate API because it performs a complete replacement
protocol rather than opening a normal write handle.

### Rooted Capabilities

`LocalRoot` anchors descendant operations to an opened directory descriptor,
which is the filesystem authority; its stored absolute path is diagnostic
context only.
Construct descendant names with `LocalRelativePath`, which accepts only a
non-empty sequence of normal relative components. `open_reader`, `open_writer`,
and `begin_atomic_write` traverse from the open root descriptor and reject
symbolic links at intermediate and final entries. Renaming or replacing the
root path or an already opened intermediate name does not redirect that handle.
The operating system resolves ancestor components in the root input before the
capability is acquired; no-follow applies to the final root entry. Containment
begins after that directory descriptor has been opened.

This is descriptor-relative path containment, not inode-name uniqueness or a
complete OS security boundary. Hard links, mounts, permissions, and processes
with equivalent OS authority remain deployment concerns. Path-based
`LocalFiles` APIs are convenience operations, not sandbox boundaries.

The secure backend currently uses Unix descriptor-relative operations. On
other targets `LocalRoot::open` returns `std::io::ErrorKind::Unsupported`
instead of falling back to a check-then-path sequence. `LocalRoot` is the API
for attacker-resistant containment; path-based `LocalFiles` and temporary
resource helpers remain intended for trusted local application paths.

### Atomic Writes

`LocalFiles::atomic_write` writes bytes to a temporary file in the same parent
directory, flushes and syncs that file, replaces the destination, and syncs the
destination parent plus the parents of directories created by the operation,
from deepest to shallowest, when supported. The destination must be absent or
an existing regular file. Symbolic links, directories, FIFOs, sockets, devices,
and other special files are rejected with
`std::io::ErrorKind::InvalidInput`. This is useful for whole-file replacement
of configuration files, cache manifests, checkpoints, and generated indexes.

Existing-target metadata preservation is strict and platform-native:

| Target | Metadata preserved from the existing destination |
| --- | --- |
| Windows | `ReplaceFileW` with flags `0`: creation time, short name, object identifier, DACL, security resource attributes, encryption, compression, and named streams absent from staging. |
| Linux / Android | uid, gid, complete Unix mode, and all descriptor-visible xattrs, including POSIX ACLs, SELinux labels, and file capabilities where exposed. |
| macOS | uid, gid, mode, ACLs, and extended attributes through descriptor APIs and `fcopyfile`. |
| FreeBSD | uid, gid, mode, the filesystem's POSIX or NFSv4 ACL, and user/system extattrs. |

This table describes implemented code paths. Android and FreeBSD belong to the
compile-only support tier defined below; their filesystem and metadata behavior
is not runtime-validated by this repository's CI.

The crate does not clear `FILE_ATTRIBUTE_READONLY` to force a Windows
replacement. A read-only destination is rejected by `ReplaceFileW` and remains
unchanged.

Unix metadata is read from an opened destination during `commit`, so changes
made after the writer begins are included. If any protected metadata cannot be
read, copied, or merged, commit fails rather than silently weakening it. Unix
does not promise to preserve inode or hard-link identity, timestamps, or
immutable/append-only flags. A target that was initially absent is installed
with a native no-replace operation, so a concurrent creator is not overwritten.
The final Unix identity check and replacement are still separate operations;
coordinate concurrent writers externally and use `LocalRootAtomicWriter` when
descriptor-relative containment is required. On Unix, a new destination starts
with mode `0600` before applying a more restrictive process umask.

For streaming content, use `LocalAtomicWriter`:

```rust
use std::io::Write;
use qubit_local_files::LocalFiles;

let mut writer = LocalFiles::begin_atomic_write("state.bin")?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalAtomicWriter` implements `Write`, but not `Seek`. Only a successful
`commit` replaces the destination; `abort` or drop leaves it unchanged and
cleans up the staging file. This existing writer remains path-based; use
`LocalRootAtomicWriter` when replacement must stay beneath an anchored root.
`atomic_write_with` lends the same guarded writer to its callback. The callback
can write the staged contents, but cannot clone, retain, seek, or access the
underlying file or raw handle after the callback returns.

Since `0.5.0`, configuration fields are private. Use the existing getters,
constructors, and builders instead of direct field access.
Failures return `LocalAtomicWriteError`, including the failed stage, temporary
path, native source error, a `LocalAtomicDestinationState`, and any secondary
error raised while removing an uncommitted staging file. `Unchanged` means the
destination was not modified; `Replaced` means it contains staged contents;
`Missing` means no destination exists; `Indeterminate` requires inspecting both
paths before recovery. Cleanup is attempted only for `Unchanged`; other states
retain any still-existing staging entry, although a successful move means the
diagnostic staging path no longer exists.
If an `atomic_write_with` callback panics, the uncommitted temporary file is
closed and best-effort removed before the panic propagates. A cleanup failure
cannot replace the panic, so the staging path may remain in that case.

The operation is not a multi-file transaction and does not coordinate concurrent
writers. Use an external lock if multiple processes or threads may replace the
same destination path at the same time.

### Recursive Directory Copy

`LocalFiles::copy_dir_all_with` copies a directory tree and returns
`LocalCopyDirStats`:

| Field | Meaning |
| --- | --- |
| `files` | Number of regular files copied. |
| `directories` | Number of destination directories created. |
| `bytes` | Number of bytes copied from regular files. |
| `skipped` | Number of existing destination files skipped. |

`LocalCopyDirOptions::default()` is intentionally conservative: both
`conflict` and `type_conflict` use `Fail`, symbolic links are not followed, and
source permissions are not preserved. On Unix, new or replaced files therefore
use mode `0600` and newly created directories use mode `0700`, before applying
a more restrictive process umask. Select `Overwrite` or `Skip` through
`LocalCopyConflictPolicy`, and opt into destructive file/directory type
replacement separately through `LocalCopyTypeConflictPolicy::Replace`. Copy
failures return `LocalCopyDirError` with paths, stage, partial statistics, the
optional staging path and secondary cleanup error, and the native source error.
`Fail` and `Skip` file commits require native no-replace support and therefore
return `Unsupported` outside Linux, macOS, and Windows. `Overwrite` uses the
ordinary replacement primitive and is not subject to that restriction.

Recursive copy is not a tree-level transaction. Entries committed before a
failure remain in the destination, no rollback is attempted, and destructive
type-conflict replacement may remove an existing destination directory before
a later operation fails.

Each copied file is validated as regular from the same opened handle that
supplies its bytes and optional permissions. Unix uses a no-follow open when
links are disabled, and Windows rejects name-surrogate reparse handles.
Directory traversal, destination rechecks, and destructive replacement remain
path-based operations, so the policy is not an attacker-resistant sandbox when
another actor can mutate either tree concurrently.

### Filename Helpers

`LocalFilenames` provides random and lexical filename utilities:

| Method group | Purpose |
| --- | --- |
| `random`, `random_with` | Build random filename components and panic on generation errors. |
| `try_random`, `try_random_with` | Build random filename components through `std::io::Result`. |
| `validate_portable_file_name` | Validate a conservative portable single-component filename. |
| `file_name`, `file_stem`, `file_prefix` | Extract UTF-8 path components using `Path` semantics. |
| `extension`, `dot_extension`, `has_extension` | Inspect final extensions. |
| `has_extension_ignore_ascii_case` | Inspect final extensions with ASCII-only case folding. |
| `file_name_from_path` | Get the final segment from a path-like string. |
| `file_name_from_url` | Get the final URL path segment, decoding safe percent-encoded UTF-8. |

The lexical helpers do not touch the filesystem. Public methods that return
filename data return UTF-8 strings instead of `OsStr`; invalid UTF-8 path
components are reported as `None`.
Portable validation rejects Windows device names that use superscript digits,
including `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²`, and `LPT³`, following
[Microsoft's file-naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

## Crate Boundary

`qubit-local-files` is intentionally limited to local filesystem concerns. It
does not provide:

- stream extension traits, binary codecs, or stream wrappers;
- asynchronous filesystem APIs or runtime integration;
- remote filesystem, FTP, S3, object storage, or VFS abstractions;
- file watching, globbing, or a general directory-walk framework;
- locking or cross-process write coordination.

For stream and byte-I/O concerns, use
[qubit-io](https://github.com/qubit-ltd/rs-io).

## Platform Support

| Support tier | Targets | CI validation |
| --- | --- | --- |
| Native runtime-tested | Linux, Windows, macOS | Tests execute on the named operating system, including platform-specific filesystem behavior. |
| Compile-only | FreeBSD, Android | CI cross-compiles the production backend and cfg-selected sources with `cargo check`; runtime filesystem, ABI, and metadata behavior are not validated or guaranteed by this repository. |

## Runtime Dependencies

This crate depends on the Rust standard library, `getrandom`, `libc`, `log`, and
the target-scoped `windows-sys` bindings. `getrandom` is used for random
temporary names. `libc` supplies Unix descriptor, metadata, ACL/extattr, and
native rename operations. `windows-sys` supplies native Windows handle and
replacement APIs. `log` is used for drop-time cleanup warnings.

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
