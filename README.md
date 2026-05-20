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
`qubit-io`:

- `LocalFiles` for parent creation, buffered file helpers, directory cleanup,
  directory size, recursive directory copy, and durable atomic writes;
- `LocalFilenames` for random and lexical file-name operations;
- `LocalTempFile` and `LocalTempDir` for RAII temporary files and directories;
- `LocalCopyDirOptions`, `LocalCopyDirStats`, and `LocalPersistOptions` for
  explicit filesystem behavior.

`qubit-io` remains focused on stream-level `std::io` traits, extension methods,
wrappers, and codecs.

## Installation

```toml
[dependencies]
qubit-local-files = "0.1"
```

## Temporary Files and Directories

`LocalTempFile` and `LocalTempDir` create real temporary filesystem entries and remove
them automatically on drop unless callers call `keep` or `persist`. Drop-time
cleanup is best-effort; failures are reported through the `log` facade with
`warn!` and never panic.

`LocalTempFile::persist` rejects an existing target by default. Use
`persist_with` and `LocalPersistOptions { overwrite: true }` when replacing an
existing target is intended.

```rust
use std::io::Write;

use qubit_local_files::{LocalPersistOptions, LocalTempDir, LocalTempFile};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

## Atomic Writes

Use `LocalFiles::atomic_write` when a file must not be observed half-written. It
writes through a temporary file in the same directory, flushes and syncs that
file, replaces the destination, and syncs the parent directory when supported.

```rust
use qubit_local_files::{LocalFiles, LocalTempDir};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-atomic-"))?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

## Main APIs

| API | Purpose |
| --- | --- |
| `LocalFiles::open_buffered_reader` | Opens a file as `BufReader<File>`. |
| `LocalFiles::ensure_dir` | Creates a directory and missing ancestors. |
| `LocalFiles::ensure_parent` | Creates missing parent directories for a file path. |
| `LocalFiles::create_file_with_parent` | Creates missing parent directories, then creates a file. |
| `LocalFiles::create_buffered_writer_with_parent` | Creates missing parent directories, then creates `BufWriter<File>`. |
| `LocalFiles::dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `LocalFiles::clean_dir` | Removes all children from a directory while keeping the directory itself. |
| `LocalFiles::remove_any` | Removes a file, directory tree, or symbolic link. |
| `LocalFiles::copy_dir_all_with` | Recursively copies a local directory tree with explicit copy options and returns copy statistics. |
| `LocalFiles::atomic_write` | Performs durable same-directory atomic file replacement. |
| `LocalFiles::atomic_write_with` | Same as `atomic_write`, but accepts caller-provided write logic. |
| `LocalTempFile` | Temporary file guard that removes the file on drop unless kept or persisted. |
| `LocalTempDir` | Temporary directory guard that removes the directory tree on drop unless kept or persisted. |
| `LocalFilenames` | Random file-name generation and lexical UTF-8 file-name helpers. |
| `LocalCopyDirOptions` | Options controlling recursive directory copy behavior. |
| `LocalCopyDirStats` | Statistics returned by recursive directory copy operations. |
| `LocalPersistOptions` | Options controlling whether temporary file persistence may overwrite an existing target. |

## Runtime Dependencies

This crate depends on the Rust standard library, `getrandom`, and `log`.
`getrandom` is used for random temporary names. `log` is used for drop-time
cleanup warnings.
