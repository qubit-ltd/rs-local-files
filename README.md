# Qubit Local FS

[![Rust CI](https://github.com/qubit-ltd/rs-local-fs/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-fs/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-fs/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-fs/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-fs.svg?color=blue)](https://crates.io/crates/qubit-local-fs)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Chinese Document](https://img.shields.io/badge/Document-Chinese-blue.svg)](README.zh_CN.md)

Local filesystem utilities for Rust.

## Overview

Qubit Local FS contains the local filesystem utilities split out of
`qubit-io`:

- `Files` for parent creation, buffered file helpers, directory cleanup,
  directory size, recursive directory copy, and durable atomic writes;
- `Filenames` for random and lexical file-name operations;
- `TempFile` and `TempDir` for RAII temporary files and directories;
- `CopyDirOptions` and `CopyDirStats` for explicit recursive copy behavior.

`qubit-io` remains focused on stream-level `std::io` traits, extension methods,
wrappers, and codecs.

## Installation

```toml
[dependencies]
qubit-local-fs = "0.1"
```

## Temporary Files and Directories

`TempFile` and `TempDir` create real temporary filesystem entries and remove
them automatically on drop unless callers call `keep` or `persist`. Drop-time
cleanup is best-effort; failures are reported through the `log` facade with
`warn!` and never panic.

```rust
use std::io::Write;

use qubit_local_fs::{TempDir, TempFile};

let dir = TempDir::with_prefix(Some("qubit-local-fs-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

let mut file = TempFile::with_name(Some("qubit-local-fs-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

## Atomic Writes

Use `Files::atomic_write` when a file must not be observed half-written. It
writes through a temporary file in the same directory, flushes and syncs that
file, replaces the destination, and syncs the parent directory when supported.

```rust
use qubit_local_fs::{Files, TempDir};

let dir = TempDir::with_prefix(Some("qubit-local-fs-atomic-"))?;
let path = dir.path().join("state").join("manifest.json");

Files::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

## Main APIs

| API | Purpose |
| --- | --- |
| `Files::open_buffered_reader` | Opens a file as `BufReader<File>`. |
| `Files::ensure_dir` | Creates a directory and missing ancestors. |
| `Files::ensure_parent` | Creates missing parent directories for a file path. |
| `Files::create_file_with_parent` | Creates missing parent directories, then creates a file. |
| `Files::create_buffered_writer_with_parent` | Creates missing parent directories, then creates `BufWriter<File>`. |
| `Files::dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `Files::clean_dir` | Removes all children from a directory while keeping the directory itself. |
| `Files::remove_any` | Removes a file, directory tree, or symbolic link. |
| `Files::copy_dir_all_with` | Recursively copies a local directory tree with explicit copy options and returns copy statistics. |
| `Files::atomic_write` | Performs durable same-directory atomic file replacement. |
| `Files::atomic_write_with` | Same as `atomic_write`, but accepts caller-provided write logic. |
| `TempFile` | Temporary file guard that removes the file on drop unless kept or persisted. |
| `TempDir` | Temporary directory guard that removes the directory tree on drop unless kept or persisted. |
| `Filenames` | Random file-name generation and lexical UTF-8 file-name helpers. |
| `CopyDirOptions` | Options controlling recursive directory copy behavior. |
| `CopyDirStats` | Statistics returned by recursive directory copy operations. |

## Runtime Dependencies

This crate depends on the Rust standard library, `getrandom`, and `log`.
`getrandom` is used for random temporary names. `log` is used for drop-time
cleanup warnings.
