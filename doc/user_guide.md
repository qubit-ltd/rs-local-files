# Qubit Local Files User Guide

Qubit Local Files is the local filesystem crate in the Qubit Rust family. It
focuses on concrete local paths, filenames, temporary filesystem entries,
recursive directory operations, and durable same-directory atomic writes. It is
intentionally not a stream codec crate or a remote filesystem abstraction.

For stream-level `std::io` traits, extension methods, wrappers, and codecs, see
[qubit-io](https://github.com/qubit-ltd/rs-io).

## When to Use This Crate

Use `qubit-local-files` when your code works with local filesystem paths rather
than generic byte streams. Typical examples include CLI tools, code generators,
cache writers, checkpoint files, local import/export jobs, unpacked work
directories, and tests that need temporary local files.

Good fits:

- Creating temporary files or directories that normally clean themselves up.
- Keeping or persisting temporary entries after successful work.
- Rejecting accidental overwrite when persisting a temporary file.
- Replacing existing files only when `LocalPersistOptions { overwrite: true }`
  is explicit.
- Creating parent directories before opening, writing, or persisting files.
- Atomically replacing a complete file through a same-directory temporary file.
- Copying local directory trees with explicit overwrite and symlink policy.
- Cleaning a directory while preserving the directory itself.
- Calculating local directory size without following symbolic links.
- Generating random filename components or validating portable filenames.

Not a fit:

- Reading, writing, comparing, limiting, or encoding arbitrary byte streams.
- Implementing binary, LEB128, ZigZag, or length-prefixed string codecs.
- Abstracting local, FTP, object storage, or remote filesystems behind one API.
- Watching files for changes.
- Coordinating concurrent writers with locks.
- Providing async filesystem APIs tied to a runtime.

Those stream and byte-I/O concerns belong in
[qubit-io](https://github.com/qubit-ltd/rs-io).

## Installation

```toml
[dependencies]
qubit-local-files = "0.1"
```

## Import Patterns

Import the concrete namespaces, guards, and option structs from the crate root:

```rust
use qubit_local_files::{
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalCopyDirOptions,
    LocalFilenames,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};
```

The crate currently does not expose a prelude. Keeping imports explicit makes
filesystem side effects and overwrite policies visible at call sites.

## Read and Write Options

Normal file opening is controlled by explicit option structs:

| Type | Fields | Purpose |
| --- | --- | --- |
| `FileReadOptions` | `buffering` | Controls whether `open_reader` returns an unbuffered or buffered reader. |
| `FileWriteOptions` | `create_parent`, `mode`, `buffering` | Controls parent creation, write mode, and writer buffering. |
| `FileBuffering` | `Unbuffered`, `Buffered { capacity }` | Selects raw file I/O or `BufReader` / `BufWriter` with an optional capacity. |
| `FileWriteMode` | enum variants | Selects how the target is opened for writing. |

Write modes:

| Mode | Behavior |
| --- | --- |
| `OpenExistingAtStart` | Opens an existing file for writing at offset zero without truncating it. |
| `CreateNew` | Creates a new file and fails when the target exists. |
| `CreateOrTruncate` | Creates a missing file or truncates an existing file. This is the default. |
| `AppendExisting` | Appends to an existing file and fails when it is missing. |
| `AppendOrCreate` | Appends to an existing file or creates it when missing. |

`LocalFiles::atomic_write` is intentionally separate from `FileWriteOptions`.
It performs a complete durable replacement protocol rather than returning a
normal write handle.

## Temporary Directories

Use `LocalTempDir` when a temporary directory should normally be cleaned up
automatically. The directory is created immediately and removed recursively when
the guard is dropped.

```rust
use qubit_local_files::LocalTempDir;

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-work-"))?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

Creation methods:

| Method | Purpose |
| --- | --- |
| `LocalTempDir::new` | Creates a temporary directory in `std::env::temp_dir()` with the default prefix. |
| `LocalTempDir::with_prefix` | Creates a temporary directory in `std::env::temp_dir()` with a custom prefix. |
| `LocalTempDir::in_dir` | Creates a temporary directory under a caller-provided parent and retry limit. |

Ownership methods:

| Method | Behavior |
| --- | --- |
| `path` | Borrows the generated directory path. |
| `exists` | Checks whether the directory path exists, returning `std::io::Result<bool>`. |
| `metadata` | Reads directory metadata. |
| `list` | Lists direct child entries. |
| `child_path` | Resolves a safe relative child path without creating it. |
| `ensure_child_dir` | Creates a child directory and missing parents, like `mkdir -p`. |
| `open_child_reader` | Opens a child file for reading with `FileReadOptions`. |
| `open_child_writer` | Opens a child file for writing with `FileWriteOptions`. |
| `cleanup` | Removes the directory immediately and disables later drop cleanup. |
| `keep` | Consumes the guard and leaves the directory at its generated path. |
| `persist` | Moves the directory to a final path and disables automatic cleanup. |

`LocalTempDir::persist` creates missing parent directories for the target and
rejects an existing target. It does not provide an overwrite option. If the move
fails, the guard still owns the temporary directory and will clean it up on drop.

Child paths must be non-empty relative paths made only of normal path
components. Absolute paths, root or prefix components, `.` and `..` are
rejected. `open_child_reader` requires the child to be a file; directories and
other non-file entries return `ErrorKind::InvalidInput`. `open_child_writer`
validates existing targets as files and keeps child writes inside the temporary
directory. `ensure_child_dir` creates missing nested parents, but rejects
symbolic link components while creating directories so the operation cannot
leave the temporary directory through a child path.

Cleanup in `Drop` is best-effort. If deletion fails, `LocalTempDir` logs a
warning through the `log` facade and does not panic.

## Temporary Files

Use `LocalTempFile` when you need a unique temporary file path with an owned
writer. The file is removed on drop unless it is kept or persisted.

```rust
use std::io::Write;

use qubit_local_files::{
    FileWriteMode,
    FileWriteOptions,
    LocalTempFile,
};

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.writer(FileWriteOptions::new(FileWriteMode::CreateOrTruncate).buffered())?
    .write_all(b"temporary payload\n")?;
file.close()?;

# Ok::<(), std::io::Error>(())
```

Creation methods:

| Method | Purpose |
| --- | --- |
| `LocalTempFile::new` | Creates a temporary file in `std::env::temp_dir()` with the default prefix. |
| `LocalTempFile::with_name` | Creates a temporary file in `std::env::temp_dir()` with custom prefix and suffix. |
| `LocalTempFile::in_dir` | Creates a temporary file under a caller-provided parent and retry limit. |

Writer and ownership methods:

| Method | Behavior |
| --- | --- |
| `path` | Borrows the generated file path. |
| `exists` | Checks whether the file path exists, returning `std::io::Result<bool>`. |
| `metadata` | Reads file metadata. |
| `writer` | Configures and returns the owned `LocalFileWriter` using `FileWriteOptions`. |
| `close` | Flushes and closes the writer while keeping path cleanup active. |
| `cleanup` | Removes the file immediately and disables later drop cleanup. |
| `keep` | Flushes, closes, consumes the guard, and leaves the file at its generated path. |
| `persist` | Moves the file to a final path without overwriting. |
| `persist_with` | Moves the file to a final path using `LocalPersistOptions`. |

The first `writer(options)` call configures the owned writer. Later calls must
pass the same options and return the same writer; different options are rejected
with `ErrorKind::InvalidInput`. Because `LocalTempFile` creates the file before
the writer is configured, `FileWriteMode::CreateNew` returns
`ErrorKind::AlreadyExists`.

`LocalTempFile` intentionally does not provide read helpers. A temporary file is
normally written, closed, then persisted. If you need to inspect its contents,
call `close` and then read `path()` through `LocalFiles::open_reader` or
`std::fs`.

`LocalTempFile::persist` flushes and closes the writer, creates missing parent
directories for the target, and rejects existing targets by using a no-clobber
move operation. It intentionally does not rely on a separate metadata precheck.
This avoids a time-of-check/time-of-use overwrite race on supported platforms.

Use `persist_with` only when the overwrite policy should differ:

```rust
use std::io::Write;

use qubit_local_files::{
    FileWriteMode,
    FileWriteOptions,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-persist-"))?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.writer(FileWriteOptions::new(FileWriteMode::CreateOrTruncate))?
    .write_all(b"new\n")?;

file.persist_with(&target, LocalPersistOptions { overwrite: true })?;

assert_eq!("new\n", std::fs::read_to_string(&target)?);

# Ok::<(), std::io::Error>(())
```

If a target file must never be observed half-written, prefer
`LocalFiles::atomic_write` for the final file replacement.

## Atomic Writes

`LocalFiles::atomic_write` writes bytes to a temporary file in the same parent
directory, flushes and syncs that temporary file, replaces the destination, and
syncs the parent directory when supported.

```rust
use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-guide-"))?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), std::io::Error>(())
```

Use `LocalFiles::atomic_write_with` when content generation needs direct access
to the temporary file handle:

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-json-"))?;
let path = dir.path().join("state.json");

LocalFiles::atomic_write_with(&path, |file| {
    writeln!(file, "{{\"complete\":true}}")
})?;

assert_eq!("{\"complete\":true}\n", std::fs::read_to_string(&path)?);

# Ok::<(), std::io::Error>(())
```

Important semantics:

- Parent directories are created before writing.
- The temporary file is created in the destination directory, so replacement can
  be atomic on common local filesystems.
- Existing regular-file permissions are copied to the temporary file before
  replacement.
- If writing, flushing, or syncing the temporary file fails, the destination is
  left untouched.
- If replacement succeeds but syncing the parent directory fails, the method may
  return an error after the destination already contains the new contents.
- If the destination path is a symbolic link on platforms where renaming over a
  symlink replaces the link itself, the link is replaced and its previous target
  is left unchanged.
- The operation is not a multi-file transaction and does not coordinate
  concurrent writers.

## File and Directory Helpers

`LocalFiles` provides small local filesystem helpers:

| Method | Behavior |
| --- | --- |
| `exists` | Checks whether a path exists without swallowing inspection errors. |
| `metadata` | Reads path metadata with `std::fs::metadata`. |
| `list` | Lists direct entries of a directory. |
| `open_reader` | Opens a file as `LocalFileReader` with `FileReadOptions`. |
| `open_writer` | Opens a file as `LocalFileWriter` with `FileWriteOptions`. |
| `ensure_dir` | Creates a directory and missing ancestors. |
| `ensure_parent` | Creates missing parent directories for a file path. Parentless paths are accepted. |
| `dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `clean_dir` | Removes all children while keeping the directory itself. |
| `remove_any` | Removes a file, directory tree, or symbolic link. |

Example:

```rust
use std::io::Write;

use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-helpers-"))?;
let path = dir.path().join("nested").join("data.txt");

let mut writer = LocalFiles::open_writer(
    &path,
    FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
        .with_parent()
        .buffered(),
)?;
writer.write_all(b"payload")?;
writer.close()?;

let mut reader = LocalFiles::open_reader(&path, FileReadOptions::buffered())?;
let mut payload = String::new();
std::io::Read::read_to_string(&mut reader, &mut payload)?;
assert_eq!("payload", payload);

assert_eq!(7, LocalFiles::dir_size(dir.path())?);
LocalFiles::clean_dir(dir.path())?;
assert_eq!(0, LocalFiles::dir_size(dir.path())?);

# Ok::<(), std::io::Error>(())
```

`dir_size` and `clean_dir` require the root path to be a directory. Symbolic
links are not followed. `remove_any` removes symbolic links as links, including
links that point to directories.

## Recursive Directory Copy

Use `LocalFiles::copy_dir_all_with` when a directory tree must be copied with an
explicit overwrite and symlink policy.

```rust
use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-copy-"))?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;

assert_eq!(1, stats.files);
assert_eq!(1, stats.directories);
assert_eq!(4, stats.bytes);

# Ok::<(), std::io::Error>(())
```

Options:

| Option | Default | Behavior |
| --- | --- | --- |
| `overwrite` | `false` | Existing destination files or non-directory entries are rejected. |
| `follow_symlinks` | `false` | Symbolic links in the source tree are rejected. |
| `preserve_permissions` | `false` | Source permissions are not copied to destination entries. |

Statistics:

| Field | Meaning |
| --- | --- |
| `files` | Number of regular files copied. |
| `directories` | Number of destination directories created. |
| `bytes` | Number of bytes copied from regular files. |

The copy operation rejects destinations inside the source tree, because copying
a directory into itself can recurse indefinitely. When symlink following is
enabled, directory cycles introduced by followed symlinks are also rejected.
Unsupported source entries return `std::io::ErrorKind::Unsupported`.

## Filename Helpers

`LocalFilenames` contains lexical helpers that do not touch the filesystem.
Methods that return filename data return UTF-8 strings (`&str` or `String`)
instead of `OsStr`; invalid UTF-8 path components are reported as `None`.

```rust
use std::path::Path;

use qubit_local_files::LocalFilenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
assert_eq!(Some("archive"), LocalFilenames::file_prefix(path));
assert_eq!(Some("gz"), LocalFilenames::extension(path));
assert_eq!(Some(".gz".to_owned()), LocalFilenames::dot_extension(path));
assert!(LocalFilenames::has_extension(path, ".gz"));
assert!(LocalFilenames::has_extension_ignore_ascii_case(path, "GZ"));

let name = LocalFilenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));
assert!(name.ends_with(".tmp"));

# Ok::<(), std::io::Error>(())
```

Use `validate_portable_file_name` when a caller-provided name should be a
conservative single path component across common platforms:

```rust
use std::io::ErrorKind;

use qubit_local_files::LocalFilenames;

LocalFilenames::validate_portable_file_name("report.csv")?;

let error = LocalFilenames::validate_portable_file_name("CON.txt")
    .expect_err("Windows reserved names are rejected");
assert_eq!(ErrorKind::InvalidInput, error.kind());

# Ok::<(), std::io::Error>(())
```

Portable validation is lexical. It does not check current filesystem
permissions, mount options, Unicode normalization, or every filesystem-specific
limit.

For strings that are not already `Path` values, use the string helpers:

```rust
use qubit_local_files::LocalFilenames;

assert_eq!("file.txt", LocalFilenames::file_name_from_path(r"C:\tmp\file.txt"));
assert_eq!(
    "report 2026.csv",
    LocalFilenames::file_name_from_url("https://example.test/files/report%202026.csv?download=1"),
);
```

`file_name_from_url` strips query and fragment suffixes before selecting the
last slash-delimited segment. It decodes percent-encoded UTF-8 only when the
decoded value remains a safe single filename fragment.

## Error and Cleanup Model

Most APIs return `std::io::Result` and preserve `std::io::ErrorKind` where
possible.

Important error behavior:

- Existing temporary-file persistence targets are rejected unless
  `LocalPersistOptions { overwrite: true }` is explicit.
- Existing temporary-directory persistence targets are rejected.
- Recursive copy rejects existing destination entries unless
  `LocalCopyDirOptions { overwrite: true, .. }` is explicit.
- Recursive copy rejects symbolic links unless
  `LocalCopyDirOptions { follow_symlinks: true, .. }` is explicit.
- Drop-time cleanup failures are logged through `log::warn!` and never panic.
- `LocalTempFile::writer` returns `ErrorKind::NotFound` after `close`.
- `LocalTempFile::writer` returns `ErrorKind::InvalidInput` when it has already
  been configured with different options.
- `LocalTempDir` child APIs return `ErrorKind::InvalidInput` for unsafe child
  paths, non-file child readers, and child paths that escape the temporary
  directory through symbolic links.

## Path Lengths and Platform Limits

`LocalTempFile` and `LocalTempDir` create local filesystem entries and return
operating system errors when creation fails. They do not promise that the
resulting path is valid for every platform API. Some APIs, such as Unix domain
sockets, have much shorter path limits than regular files. For those cases,
create temporary entries under a short parent directory such as `/tmp`.

## Crate Boundary

`qubit-local-files` deliberately keeps local filesystem utilities out of
`qubit-io`. Use this crate for local paths, temporary files and directories,
recursive directory operations, directory cleanup, filename helpers, and atomic
file writes.

Use [qubit-io](https://github.com/qubit-ltd/rs-io) when you need stream traits,
extension methods, stream wrappers, content comparison, bounded reads, or binary
codecs.

## Testing and CI

The project includes tests for public helpers, temporary entries, overwrite
semantics, recursive copy behavior, filename validation, atomic writes, and
platform-sensitive edge cases.

Useful commands:

```bash
cargo test
./coverage.sh
./coverage.sh text
./ci-check.sh
```
