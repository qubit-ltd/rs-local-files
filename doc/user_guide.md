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
| `keep` | Consumes the guard and leaves the directory at its generated path. |
| `persist` | Moves the directory to a final path and disables automatic cleanup. |

`LocalTempDir::persist` creates missing parent directories for the target and
rejects an existing target. It does not provide an overwrite option. If the move
fails, the guard still owns the temporary directory and will clean it up on drop.

Cleanup in `Drop` is best-effort. If deletion fails, `LocalTempDir` logs a
warning through the `log` facade and does not panic.

## Temporary Files

Use `LocalTempFile` when you need both a unique path and an already-open file
handle. The file is removed on drop unless it is kept or persisted.

```rust
use std::io::Write;

use qubit_local_files::LocalTempFile;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
writeln!(file.file_mut()?, "temporary payload")?;

# Ok::<(), std::io::Error>(())
```

Creation methods:

| Method | Purpose |
| --- | --- |
| `LocalTempFile::new` | Creates a temporary file in `std::env::temp_dir()` with the default prefix. |
| `LocalTempFile::with_name` | Creates a temporary file in `std::env::temp_dir()` with custom prefix and suffix. |
| `LocalTempFile::in_dir` | Creates a temporary file under a caller-provided parent and retry limit. |

Handle and ownership methods:

| Method | Behavior |
| --- | --- |
| `path` | Borrows the generated file path. |
| `file` | Borrows the open file handle. |
| `file_mut` | Mutably borrows the open file handle. |
| `close` | Closes the handle while keeping path cleanup active. |
| `keep` | Consumes the guard and leaves the file at its generated path. |
| `persist` | Moves the file to a final path without overwriting. |
| `persist_with` | Moves the file to a final path using `LocalPersistOptions`. |

`LocalTempFile::persist` closes the file handle, creates missing parent
directories for the target, and rejects existing targets by using a no-clobber
move operation. It intentionally does not rely on a separate metadata precheck.
This avoids a time-of-check/time-of-use overwrite race on supported platforms.

Use `persist_with` only when the overwrite policy should differ:

```rust
use std::io::Write;

use qubit_local_files::{
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-persist-"))?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
writeln!(file.file_mut()?, "new")?;

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
| `open_buffered_reader` | Opens a file as `BufReader<File>`. |
| `ensure_dir` | Creates a directory and missing ancestors. |
| `ensure_parent` | Creates missing parent directories for a file path. Parentless paths are accepted. |
| `create_file_with_parent` | Creates missing parent directories, then creates a file. |
| `create_buffered_writer_with_parent` | Creates missing parent directories, then creates `BufWriter<File>`. |
| `dir_size` | Sums regular-file byte lengths below a directory without following symbolic links. |
| `clean_dir` | Removes all children while keeping the directory itself. |
| `remove_any` | Removes a file, directory tree, or symbolic link. |

Example:

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-helpers-"))?;
let path = dir.path().join("nested").join("data.txt");

let mut writer = LocalFiles::create_buffered_writer_with_parent(&path)?;
writer.write_all(b"payload")?;
drop(writer);

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
- `LocalTempFile::file` and `file_mut` return `ErrorKind::NotFound` after the
  handle has been closed or released.

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
