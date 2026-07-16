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
- Replacing existing files only when
  `LocalPersistOptions::new().with_overwrite()`
  is explicit.
- Creating parent directories before opening, writing, or persisting files.
- Atomically replacing a complete file through a same-directory temporary file.
- Copying local directory trees with explicit conflict and symlink policy.
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
qubit-local-files = "0.5"
```

## Import Patterns

Import the concrete namespaces, guards, and option structs from the crate root:

```rust
use qubit_local_files::{
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyTypeConflictPolicy,
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
| `FileBuffering` | `Unbuffered`, `Buffered { capacity }` | Selects raw file I/O or `BufReader` / `BufWriter` with an optional non-zero capacity. |
| `FileWriteMode` | enum variants | Selects how the target is opened for writing. |

Readers returned by `LocalFiles::open_reader` implement `Read` and `Seek`.
Writers returned by `LocalFiles::open_writer` implement `Write` and `Seek`.
Both helpers return only regular files. They reject directories, FIFOs,
sockets, and other special filesystem resources; Unix FIFO rejection does not
wait for a peer.
`LocalFileWriter::sync_all` and `LocalFileWriter::sync_data` flush any buffered
bytes before synchronizing the underlying file, which is useful for append logs
or other normal write handles that do not need whole-file atomic replacement.
Seeking a writer does not disable append-mode semantics.

`FileBuffering::buffered_with_capacity`,
`FileReadOptions::buffered_with_capacity`, and
`FileWriteOptions::buffered_with_capacity` return `std::io::Result` and reject
zero. A successfully constructed custom capacity is stored as `NonZeroUsize`,
so file-opening methods cannot receive an invalid zero-capacity policy.

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
| `path` | Borrows the generated absolute directory path. |
| `exists` | Checks whether the directory path exists, returning `std::io::Result<bool>`. |
| `metadata` | Reads directory metadata. |
| `list` | Lists direct child entries. |
| `child_path` | Lexically validates a relative child and returns its absolute joined path without inspecting the filesystem. |
| `ensure_child_dir` | Creates a child directory and missing parents, like `mkdir -p`, and returns its absolute path. |
| `open_child_reader` | Opens a child file for reading with `FileReadOptions`. |
| `open_child_writer` | Opens a child file for writing with `FileWriteOptions`. |
| `cleanup` | Removes the directory immediately and disables later drop cleanup. |
| `keep` | Consumes the guard, leaves the directory in place, and returns its absolute path. |
| `persist` | Moves the directory to a final path, returns its absolute path, and disables automatic cleanup. |

`LocalTempDir::persist` creates missing parent directories for the target and
rejects an existing target. It does not provide an overwrite option. If the move
fails, `LocalPersistError` returns ownership of the guard so callers can retry,
keep, inspect, or explicitly clean up the directory.
Persistence uses a native move/rename without a copy-and-delete fallback, so a
cross-filesystem move may fail with `EXDEV` on Unix or an equivalent platform
error.

Child paths must be non-empty relative paths made only of normal path
components. Absolute paths, root or prefix components, `.` and `..` are
rejected. `child_path` stops after this lexical validation: an existing
symbolic-link component may still resolve outside the temporary directory, so
the returned path is not proof of filesystem containment. `open_child_reader`
requires the child to be a file; directories and
other non-file entries return `ErrorKind::InvalidInput`. `open_child_writer`
validates existing targets as files and keeps child writes inside the temporary
directory. `ensure_child_dir` creates missing nested parents, but rejects
symbolic link components while creating directories so the operation cannot
leave the temporary directory through a child path.

These checks assume an untrusted actor is not replacing path components between
validation and use. The child helpers are convenience containment checks, not a
capability-based sandbox boundary for concurrent filesystem mutation.

Cleanup in `Drop` is best-effort. If deletion fails, `LocalTempDir` logs a
warning through the `log` facade and does not panic.

## Temporary Files

Use `LocalTempFile` when you need a unique temporary file path with an owned
file handle. The file is removed on drop unless it is kept or persisted. On
Unix, temporary files are created with mode `0600` and temporary directories
with mode `0700`, before applying the process umask.

```rust
use std::io::Write;

use qubit_local_files::LocalTempFile;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.write_all(b"temporary payload\n")?;
file.close();

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
| `path` | Borrows the generated absolute file path. |
| `exists` | Checks whether the file path exists, returning `std::io::Result<bool>`. |
| `metadata` | Reads file metadata. |
| `as_file` / `as_file_mut` | Borrows the original owned `File` handle. |
| `Write` / `Seek` | Writes or seeks directly through the owned handle. |
| `close` | Drops the unbuffered handle while keeping path cleanup active; it does not call `sync_all`. |
| `cleanup` | Removes the file immediately and disables later drop cleanup. |
| `keep` | Closes and consumes the guard, leaving the file in place and returning its absolute path. |
| `persist` | Moves the file without overwriting and returns the absolute final path. |
| `persist_with` | Moves the file using `LocalPersistOptions` and returns the absolute final path. |

`LocalTempFile` intentionally does not provide read helpers. A temporary file is
normally written, closed, then persisted. If you need to inspect its contents,
call `close` and then read `path()` through `LocalFiles::open_reader` or
`std::fs`.

`LocalTempFile::persist` closes the file, creates missing parent
directories for the target, and rejects existing targets by using a no-clobber
move operation. It intentionally does not rely on a separate metadata precheck.
This avoids a time-of-check/time-of-use overwrite race on supported platforms.
On failure it returns `LocalPersistError<LocalTempFile>`, retaining the guard and
native I/O error.

File persistence uses a native move/rename without a copy-and-delete fallback,
so cross-filesystem moves may fail with `EXDEV` on Unix or an equivalent
platform error. With overwrite enabled, the resulting file keeps the temporary
file's permissions rather than the replaced target's permissions. Use
`LocalFiles::atomic_write` when existing regular-file permissions must be
preserved while replacing contents.

Use `persist_with` only when the overwrite policy should differ:

```rust
use std::io::Write;

use qubit_local_files::{LocalPersistOptions, LocalTempDir, LocalTempFile};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-persist-"))?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
file.write_all(b"new\n")?;

file.persist_with(&target, LocalPersistOptions::new().with_overwrite())?;

assert_eq!("new\n", std::fs::read_to_string(&target)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

If a target file must never be observed half-written, prefer
`LocalFiles::atomic_write` for the final file replacement.

## Atomic Writes

`LocalFiles::atomic_write` writes bytes to a temporary file in the same parent
directory, flushes and syncs that temporary file, replaces the destination, and
syncs the destination parent plus the parents of directory entries created by
the operation, from deepest to shallowest, when supported.

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

# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `LocalFiles::atomic_write_with` when content generation should run inside a
guarded atomic-write callback. The callback receives `LocalAtomicWriter`, which
supports `Write` but cannot clone or retain the underlying file handle:

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix(Some("qubit-local-files-json-"))?;
let path = dir.path().join("state.json");

LocalFiles::atomic_write_with(&path, |writer| {
    writeln!(writer, "{{\"complete\":true}}")
})?;

assert_eq!("{\"complete\":true}\n", std::fs::read_to_string(&path)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `LocalAtomicWriter` when content should be streamed across multiple calls:

```rust
use std::io::Write;
use qubit_local_files::LocalFiles;

let mut writer = LocalFiles::begin_atomic_write("state.bin")?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalAtomicWriter` implements `Write`, but not `Seek`. Only `commit` replaces
the destination. Calling `abort` or dropping the writer preserves the original
destination and cleans up the staging file. The API remains synchronous.

Since `0.5.0`, configuration fields are private. Callers must use the existing
getters, constructors, and builders.

Important semantics:

- Parent directories are created before writing.
- The temporary file is created in the destination directory, so replacement can
  be atomic on common local filesystems.
- Existing regular-file permissions are copied to the temporary file before
  replacement. Symbolic-link targets do not donate permissions.
- On Unix, a new destination uses mode `0600`, subject to a more restrictive
  process umask.
- If writing, flushing, or syncing the temporary file fails, the destination is
  left untouched.
- If an `atomic_write_with` callback panics, unwinding closes and best-effort
  removes the uncommitted temporary file before the panic continues; the
  destination is left untouched. Cleanup failure cannot replace the panic, so
  the staging path may remain.
- If replacement succeeds but synchronizing the destination parent or a parent
  of a newly created directory entry fails, the method may return an error after
  the destination already contains the new contents.
- Errors are reported as `LocalAtomicWriteError`, which exposes the failed
  stage, temporary path, native I/O source, a `committed` flag, and any
  secondary staging cleanup error.
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
| `open_reader` | Opens a regular file as `LocalFileReader` with `FileReadOptions`; rejects directories and special resources. |
| `open_writer` | Opens or creates a regular file as `LocalFileWriter` with `FileWriteOptions`; rejects directories and special resources. |
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
explicit conflict and symlink policy.

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

# Ok::<(), Box<dyn std::error::Error>>(())
```

Options:

| Option | Default | Behavior |
| --- | --- | --- |
| `with_conflict(...)` | `Fail` | Existing destination files are rejected; choose `Overwrite` or `Skip` explicitly. |
| `with_type_conflict(...)` | `Fail` | File/directory type mismatches are rejected; `Replace` explicitly permits destructive replacement. |
| `follow_symlinks()` | `false` | Symbolic links in the source tree are rejected. |
| `preserve_permissions()` | `false` | Source permissions are not copied; on Unix, new or replaced files keep mode `0600` and new directories use `0700`, subject to the process umask. |

Statistics:

| Field | Meaning |
| --- | --- |
| `files` | Number of regular files copied. |
| `directories` | Number of destination directories created. |
| `bytes` | Number of bytes copied from regular files. |
| `skipped` | Number of existing destination files skipped. |

The copy operation rejects destinations inside the source tree, because copying
a directory into itself can recurse indefinitely. When symlink following is
enabled, directory cycles introduced by followed symlinks are also rejected.
Unsupported source entries report `std::io::ErrorKind::Unsupported` through
`LocalCopyDirError`. The structured error also exposes the failed stage, source
and destination paths, partial statistics, optional staging path, optional
secondary cleanup error, and native I/O source error. The original copy or
commit failure remains the primary source error.
The copy is not a tree-level transaction: entries committed before a failure
remain in the destination, no rollback is attempted, and destructive
type-conflict replacement may remove an existing destination directory before
a later operation fails.

Source checks, source opens, destination rechecks, and destructive replacement
are separate path-based operations. The symlink policy prevents accidental
traversal; it is not an attacker-resistant sandbox when another actor can
mutate either tree concurrently.

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
limit. It also rejects `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²`, and `LPT³`:
Windows treats the ISO/IEC 8859-1 superscript digits as device-name digits, as
documented in [Microsoft's file-naming rules](https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).

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

Simple APIs return `std::io::Result` and preserve the native error chain.
Atomic writes, recursive copy, and temporary-resource persistence use structured
errors carrying the additional state needed for safe recovery.

Important error behavior:

- Existing temporary-file persistence targets are rejected unless
  `LocalPersistOptions::new().with_overwrite()` is explicit.
- Existing temporary-directory persistence targets are rejected.
- Recursive copy uses an explicit `LocalCopyConflictPolicy` for existing files
  and a separate `LocalCopyTypeConflictPolicy` for file/directory mismatches.
- Recursive copy rejects symbolic links unless
  `LocalCopyDirOptions::new().follow_symlinks()` is explicit.
- Drop-time cleanup failures are logged through `log::warn!` and never panic.
- `LocalTempFile::as_file`, `as_file_mut`, `Write`, and `Seek` return
  `ErrorKind::NotFound` after `close`.
- `LocalTempDir` child APIs return `ErrorKind::InvalidInput` for unsafe child
  paths, non-file child readers, and child paths that escape the temporary
  directory through symbolic links.

## Path Lengths and Platform Limits

`LocalTempFile` and `LocalTempDir` create local filesystem entries and return
operating system errors when creation fails. They do not promise that the
resulting path is valid for every platform API. Some APIs, such as Unix domain
sockets, have much shorter path limits than regular files. For those cases,
create temporary entries under a short parent directory such as `/tmp`.

Relative inputs used by temporary resources and atomic writers are bound to the
process current directory when the resource or operation begins. Temporary
resource `path`, child-path, `keep`, and persistence methods return absolute
paths that remain directly usable after later current-directory changes. The
crate rejects interior UTF-16 NULs on Windows but does not add a verbatim-path
prefix, so native path-length and verbatim-path semantics still apply.

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
./align-ci.sh
./ci-check.sh
```
