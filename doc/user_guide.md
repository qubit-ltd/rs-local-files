# Qubit Local FS User Guide

Qubit Local FS provides local filesystem utilities split out of `qubit-io`. It
contains helpers for local paths, file names, temporary filesystem entries,
recursive directory operations, and durable atomic writes.

## Imports

```rust
use qubit_local_fs::{
    CopyDirOptions,
    Filenames,
    Files,
    TempDir,
    TempFile,
};
```

## Temporary Directories

Use `TempDir` when a temporary directory should normally be cleaned up
automatically. The directory is created immediately and removed recursively when
the guard is dropped. Call `keep` to keep the generated location or `persist` to
move the directory to a final path.

```rust
use qubit_local_fs::TempDir;

let dir = TempDir::with_prefix(Some("qubit-local-fs-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

Cleanup in `Drop` is best-effort. If deletion fails, `TempDir` logs a warning
through the `log` facade and does not panic.

## Temporary Files

Use `TempFile` when you need both a unique path and an already-open file handle.
The file is removed on drop unless it is kept or persisted.

```rust
use std::io::Write;

use qubit_local_fs::TempFile;

let mut file = TempFile::with_name(Some("qubit-local-fs-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

`TempFile::persist` closes the file handle before moving the temporary file to
its final path. Use `Files::atomic_write` instead when a target file must never
be observed half-written.

## Atomic Writes

`Files::atomic_write` writes to a temporary file in the same parent directory,
flushes and syncs that temporary file, replaces the destination, and syncs the
parent directory when supported.

```rust
use qubit_local_fs::{Files, TempDir};

let dir = TempDir::with_prefix(Some("qubit-local-fs-guide-"))?;
let path = dir.path().join("state").join("manifest.json");

Files::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

Use `Files::atomic_write_with` when content generation needs direct access to
the temporary file handle.

## Directory Helpers

`Files` provides local directory helpers:

- `ensure_dir` creates a directory and missing ancestors;
- `ensure_parent` creates missing parent directories for a file path;
- `dir_size` sums regular-file byte lengths without following symbolic links;
- `clean_dir` removes all children while keeping the directory itself;
- `remove_any` removes a file, directory tree, or symbolic link;
- `copy_dir_all_with` recursively copies a directory tree with explicit options.

```rust
use qubit_local_fs::{CopyDirOptions, Files, TempDir};

let dir = TempDir::with_prefix(Some("qubit-local-fs-copy-"))?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

Files::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = Files::copy_dir_all_with(&src, &dst, CopyDirOptions::default())?;
assert_eq!(1, stats.files);

# Ok::<(), std::io::Error>(())
```

## Filename Helpers

`Filenames` contains lexical helpers that do not touch the filesystem. Methods
return UTF-8 strings (`&str` or `String`) instead of `OsStr`; invalid UTF-8 path
components are reported as `None`.

```rust
use std::path::Path;

use qubit_local_fs::Filenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), Filenames::file_stem(path));
assert_eq!(Some("gz"), Filenames::extension(path));
assert!(Filenames::has_extension(path, ".gz"));

let name = Filenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));

# Ok::<(), std::io::Error>(())
```

## Path Lengths and Platform Limits

`TempFile` and `TempDir` create local filesystem entries and return operating
system errors when creation fails. They do not promise that the resulting path is
valid for every platform API. Some APIs, such as Unix domain sockets, have much
shorter path limits than regular files. For those cases, create temporary
entries under a short parent directory such as `/tmp`.
