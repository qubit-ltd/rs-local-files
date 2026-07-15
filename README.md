# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Chinese Document](https://img.shields.io/badge/Document-Chinese-blue.svg)](README.zh_CN.md)

Local filesystem utilities for Rust.

## Overview

Qubit Local Files contains the local filesystem utilities split out of
`qubit-io`. It is focused on concrete local paths and local filesystem entries:
temporary files and directories, filename helpers, recursive directory
operations, and durable same-directory atomic writes.

Use this crate when you need:

- RAII temporary files or directories that clean themselves up on drop;
- parent-directory creation before opening or writing local files;
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
qubit-local-files = "0.3"
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

let work = LocalTempDir::with_prefix(Some("qubit-local-files-readme-"))?;
let src = work.path().join("src");
let dst = work.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("manifest.json"), br#"{"version":1}"#)?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

LocalFiles::atomic_write(dst.join("manifest.json"), br#"{"version":2}"#)?;

let final_path = work.path().join("result.txt");
std::fs::write(&final_path, "old payload")?;

let mut temp = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
temp.write_all(b"new payload\n")?;
temp.persist_with(&final_path, LocalPersistOptions { overwrite: true })?;

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
| `open_reader` | Opens a file as `LocalFileReader` using `FileReadOptions`. |
| `open_writer` | Opens a file as `LocalFileWriter` using `FileWriteOptions`. |
| `ensure_dir` | Creates a directory and missing ancestors. |
| `ensure_parent` | Creates missing parent directories for a file path. |
| `dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `clean_dir` | Removes all children from a directory while keeping the directory itself. |
| `remove_any` | Removes a file, directory tree, or symbolic link. |
| `copy_dir_all_with` | Recursively copies a local directory tree with explicit options and returns statistics. |
| `atomic_write` | Replaces a file through a durable same-directory temporary write. |
| `atomic_write_with` | Same as `atomic_write`, but accepts caller-provided write logic. |

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

`LocalTempDir` provides safe child helpers: `child_path`, `ensure_child_dir`,
`open_child_reader`, `open_child_writer`, and `list`. Child paths must be
relative and cannot contain parent traversal. `ensure_child_dir` creates nested
parents like `mkdir -p`.

Child helpers reject lexical traversal and observed symbolic-link escapes, but
validation is not atomic with later filesystem operations. They are not a
sandbox boundary when an untrusted actor can mutate the tree concurrently.

`LocalTempFile::persist` rejects an existing target by default during the move
operation. Use `LocalTempFile::persist_with` and
`LocalPersistOptions { overwrite: true }` only when replacing an existing target
is intended. `LocalTempDir::persist` also rejects an existing target and does not
provide an overwrite option. A failed persistence operation returns
`LocalPersistError`, which retains the temporary guard for retry or inspection.
Persistence uses native move/rename operations without a copy-and-delete
fallback, so cross-filesystem moves may fail with `EXDEV` on Unix or an
equivalent platform error. Overwriting a file keeps the temporary file's
permissions rather than the replaced target's permissions; use
`LocalFiles::atomic_write` when replacing contents while preserving an existing
regular file's permissions is required.
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

`LocalFileReader` implements `Read` and `Seek`. `LocalFileWriter` implements
`Write` and `Seek`, and provides `sync_all` / `sync_data` helpers that flush any
buffered bytes before synchronizing the underlying file. Seeking a writer does
not disable append-mode semantics.

Custom-capacity constructors return `std::io::Result` and reject zero. The
stored custom capacity is a `NonZeroUsize`, so invalid buffering policies cannot
be passed to file-opening methods.

`atomic_write` remains a separate API because it performs a complete replacement
protocol rather than opening a normal write handle.

### Atomic Writes

`LocalFiles::atomic_write` writes bytes to a temporary file in the same parent
directory, flushes and syncs that file, replaces the destination, and syncs the
parent directory when supported. This is useful for whole-file replacement of
configuration files, cache manifests, checkpoints, and generated indexes.
Failures return `LocalAtomicWriteError`, including the failed stage, temporary
path, native source error, and whether replacement had already committed.
If an `atomic_write_with` callback panics, the panic is propagated after the
uncommitted temporary file has been closed and removed.

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
source permissions are not preserved. Select `Overwrite` or `Skip` through
`LocalCopyConflictPolicy`, and opt into destructive file/directory type
replacement separately through `LocalCopyTypeConflictPolicy::Replace`. Copy
failures return `LocalCopyDirError` with paths, stage, partial statistics, and
the native source error.

Recursive copy is not a tree-level transaction. Entries committed before a
failure remain in the destination, no rollback is attempted, and destructive
type-conflict replacement may remove an existing destination directory before
a later operation fails.

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

## Runtime Dependencies

This crate depends on the Rust standard library, `getrandom`, `libc`, and `log`
at runtime. `getrandom` is used for random temporary names. `libc` is used for
Linux no-replace rename support. `log` is used for drop-time cleanup warnings.

## Testing & Code Coverage

This project maintains test coverage for temporary file and directory cleanup,
overwrite behavior, atomic writes, recursive copy behavior, filename helpers,
and public filesystem utilities.

### Running Tests

```bash
# Run all tests
cargo test

# Run with coverage report
./coverage.sh

# Generate text format report
./coverage.sh text

# Run CI checks (format, clippy, test, coverage, audit)
./ci-check.sh
```

## License

Copyright (c) 2026. Haixing Hu.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

See [LICENSE](LICENSE) for the full license text.

## Contributing

Contributions are welcome. Please feel free to submit a Pull Request.

### Development Guidelines

- Follow the Rust API guidelines.
- Keep local filesystem concerns in `qubit-local-files`.
- Use [qubit-io](https://github.com/qubit-ltd/rs-io) for stream and byte-I/O utilities.
- Keep conservative defaults for operations that may overwrite data or leave the requested source tree.
- Maintain comprehensive test coverage for platform-sensitive filesystem behavior.
- Document public APIs with examples when they clarify behavior.
- Ensure `./ci-check.sh` passes before submitting a PR.

## Author

**Haixing Hu**

## Related Projects

- [qubit-io](https://github.com/qubit-ltd/rs-io): stream and byte-I/O utilities for Rust.
- More Rust libraries from Qubit are published under the [qubit-ltd](https://github.com/qubit-ltd) organization on GitHub.

---

Repository: [https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
