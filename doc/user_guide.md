# Qubit Local Files User Guide

Qubit Local Files provides local filesystem utilities split out of `qubit-io`. It
contains helpers for local paths, file names, temporary filesystem entries,
recursive directory operations, and durable atomic writes.

## Imports

```rust
use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFilenames,
    LocalFiles,
    LocalTempDir,
    LocalTempFile,
};
```

## Temporary Directories

Use `LocalTempDir` when a temporary directory should normally be cleaned up
automatically. The directory is created immediately and removed recursively when
the guard is dropped. Call `keep` to keep the generated location or `persist` to
move the directory to a final path.

```rust
use qubit_local_files::LocalTempDir;

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

Cleanup in `Drop` is best-effort. If deletion fails, `LocalTempDir` logs a warning
through the `log` facade and does not panic.

## Temporary Files

Use `LocalTempFile` when you need both a unique path and an already-open file handle.
The file is removed on drop unless it is kept or persisted.

```rust
use std::io::Write;

use qubit_local_files::LocalTempFile;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

`LocalTempFile::persist` closes the file handle before moving the temporary file to
its final path. Use `LocalFiles::atomic_write` instead when a target file must never
be observed half-written.

## Atomic Writes

`LocalFiles::atomic_write` writes to a temporary file in the same parent directory,
flushes and syncs that temporary file, replaces the destination, and syncs the
parent directory when supported.

```rust
use qubit_local_files::{LocalFiles, LocalTempDir};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-guide-"))?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

Use `LocalFiles::atomic_write_with` when content generation needs direct access to
the temporary file handle.

## Directory Helpers

`LocalFiles` provides local directory helpers:

- `ensure_dir` creates a directory and missing ancestors;
- `ensure_parent` creates missing parent directories for a file path;
- `dir_size` sums regular-file byte lengths without following symbolic links;
- `clean_dir` removes all children while keeping the directory itself;
- `remove_any` removes a file, directory tree, or symbolic link;
- `copy_dir_all_with` recursively copies a directory tree with explicit options.

```rust
use qubit_local_files::{LocalCopyDirOptions, LocalFiles, LocalTempDir};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-copy-"))?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

# Ok::<(), std::io::Error>(())
```

## Filename Helpers

`LocalFilenames` contains lexical helpers that do not touch the filesystem. Methods
return UTF-8 strings (`&str` or `String`) instead of `OsStr`; invalid UTF-8 path
components are reported as `None`.

```rust
use std::path::Path;

use qubit_local_files::LocalFilenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
assert_eq!(Some("gz"), LocalFilenames::extension(path));
assert!(LocalFilenames::has_extension(path, ".gz"));

let name = LocalFilenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));

# Ok::<(), std::io::Error>(())
```

## Path Lengths and Platform Limits

`LocalTempFile` and `LocalTempDir` create local filesystem entries and return operating
system errors when creation fails. They do not promise that the resulting path is
valid for every platform API. Some APIs, such as Unix domain sockets, have much
shorter path limits than regular files. For those cases, create temporary
entries under a short parent directory such as `/tmp`.
