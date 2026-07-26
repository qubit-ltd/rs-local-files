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
  `temp::PersistOptions::new().with_overwrite()`
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
qubit-local-files = "0.7"
```

## Import Patterns

Import focused modules and the concrete guard types needed by each operation:

```rust
use qubit_local_files::{
    atomic,
    copy,
    directory,
    metadata,
    path,
    read,
    remove,
    rename,
    rooted,
    temp,
    write,
};
```

The crate currently does not expose a prelude. Keeping imports explicit makes
filesystem side effects and overwrite policies visible at call sites.

## Read and Write Options

Normal file opening is controlled by explicit option structs:

| Type | Fields | Purpose |
| --- | --- | --- |
| `read::OpenOptions` | `open_retry_timeout` | Controls the optional Unix lease-conflict timeout for an unbuffered native reader. |
| `write::OpenOptions` | `create_parents`, `mode`, `open_retry_timeout` | Controls parent creation, native write mode, and the optional Unix lease-conflict timeout. |
| `write::Mode` | enum variants | Selects how the target is opened for writing. |

`read::open`, `read::open_with`, and `write::open` return unbuffered
`std::fs::File` handles.
Both helpers return only regular files. They reject directories, FIFOs,
sockets, and other special filesystem resources; Unix FIFO rejection does not
wait for a peer.

On Unix, a file lease can make the defensive nonblocking open return
`WouldBlock`. The normal open helpers retry it to preserve ordinary
blocking-open behavior. `with_open_retry_timeout` bounds that wait; the
default is unbounded and `Duration::ZERO` returns `TimedOut` after the first
lease-conflicting attempt. Other open errors are never retried. The option
applies to configured focused file-open helpers, not to later reads or writes.
Use `read::open` for the default policy and `read::open_with` when a bounded
retry timeout is required.

Write modes:

| Mode | Behavior |
| --- | --- |
| `OpenExistingAtStart` | Opens an existing file for writing at offset zero without truncating it. |
| `CreateNew` | Creates a new file and fails when the target exists. |
| `CreateOrTruncate` | Creates a missing file or truncates an existing file. This is the default. |
| `AppendExisting` | Appends to an existing file and fails when it is missing. |
| `AppendOrCreate` | Appends to an existing file or creates it when missing. |

`atomic::write` is intentionally separate from `write::OpenOptions`.
It performs a complete durable replacement protocol rather than returning a
normal write handle.

## Temporary Directories

Use `temp::TempDir` when a temporary directory should normally be cleaned up
automatically. The directory is created immediately and removed recursively when
the guard is dropped.

```rust
use qubit_local_files::temp;

let dir = temp::TempDir::with_prefix("qubit-local-files-work-")?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

Creation methods:

| Method | Purpose |
| --- | --- |
| `temp::TempDir::new` | Creates a temporary directory in `std::env::temp_dir()` with the default prefix. |
| `temp::TempDir::with_prefix` | Creates a temporary directory in `std::env::temp_dir()` with a custom prefix. |
| `temp::TempDir::in_dir` | Creates a temporary directory under a caller-provided parent and retry limit. |

Ownership methods:

| Method | Behavior |
| --- | --- |
| `path` | Borrows the generated absolute directory path. |
| `exists` | Checks whether the directory path exists, returning `std::io::Result<bool>`. |
| `metadata` | Reads directory metadata. |
| `list` | Lists direct child entries. |
| `child_path` | Lexically validates a relative child and returns its absolute joined path without inspecting the filesystem. |
| `ensure_child_dir` | Creates a child directory and missing parents, like `mkdir -p`, and returns its absolute path. |
| `open_child_reader` | Opens a child file for reading with default options. |
| `open_child_reader_with` | Opens a child file for reading with `read::OpenOptions`. |
| `open_child_writer` | Opens a child file for writing with `write::OpenOptions`. |
| `cleanup` | Removes the directory immediately and disables later drop cleanup. |
| `keep` | Consumes the guard, leaves the directory in place, and returns its absolute path. |
| `persist` | Moves the directory to a final path, returns its absolute path, and disables automatic cleanup. |

`temp::TempDir::persist` creates missing parent directories for the target and
rejects an existing target. It does not provide an overwrite option. If the move
fails, `temp::PersistError` returns ownership of the guard so callers can retry,
keep, inspect, or explicitly clean up the directory. It also reports the
failure stage, caller-requested target, and resolved absolute target when
resolution succeeded.
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

Cleanup in `Drop` is best-effort. If deletion fails, `temp::TempDir` logs a
warning through the `log` facade and does not panic.

## Temporary Files

Use `temp::TempFile` when you need a unique temporary file path with an owned
file handle. The file is removed on drop unless it is kept or persisted. On
Unix, temporary files are created with mode `0600` and temporary directories
with mode `0700`, before applying the process umask.

```rust
use std::io::Write;

use qubit_local_files::temp;

let mut file = temp::TempFile::with_affixes("qubit-local-files-", ".txt")?;
file.write_all(b"temporary payload\n")?;
file.close();

# Ok::<(), std::io::Error>(())
```

Creation methods:

| Method | Purpose |
| --- | --- |
| `temp::TempFile::new` | Creates a temporary file in `std::env::temp_dir()` with the default prefix. |
| `temp::TempFile::with_prefix` | Creates a temporary file in `std::env::temp_dir()` with a custom prefix. |
| `temp::TempFile::with_suffix` | Creates a temporary file in `std::env::temp_dir()` with the default prefix and a custom suffix. |
| `temp::TempFile::with_affixes` | Creates a temporary file in `std::env::temp_dir()` with a custom prefix and suffix. |
| `temp::TempFile::in_dir` | Creates a temporary file under a caller-provided parent and retry limit. |

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
| `persist_with` | Moves the file using `temp::PersistOptions` and returns the absolute final path. |

`temp::TempFile` intentionally does not provide read helpers. A temporary file is
normally written, closed, then persisted. If you need to inspect its contents,
call `close` and then read `path()` through `read::open` or `std::fs`.

`temp::TempFile::persist` closes the file, creates missing parent
directories for the target, and rejects existing targets by using a no-clobber
move operation. It intentionally does not rely on a separate metadata precheck.
This avoids a time-of-check/time-of-use overwrite race on supported platforms.
On failure it returns `temp::PersistError<temp::TempFile>`, retaining the guard,
native I/O error, `ResolveTarget` / `PrepareParent` / `InstallDestination`
stage, requested target, and optional resolved absolute target.

File persistence uses a native move/rename without a copy-and-delete fallback,
so cross-filesystem moves may fail with `EXDEV` on Unix or an equivalent
platform error. With overwrite enabled, the resulting file keeps the temporary
file's metadata rather than the replaced target's metadata. Use
`atomic::write` when supported platform-native metadata must be
strictly preserved while replacing contents.

Use `persist_with` only when the overwrite policy should differ:

```rust
use std::io::Write;

use qubit_local_files::temp;

let dir = temp::TempDir::with_prefix("qubit-local-files-persist-")?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = temp::TempFile::with_affixes("qubit-local-files-", ".txt")?;
file.write_all(b"new\n")?;

file.persist_with(&target, temp::PersistOptions::new().with_overwrite())?;

assert_eq!("new\n", std::fs::read_to_string(&target)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

If a target file must never be observed half-written, prefer
`atomic::write` for the final file replacement.

## No-Replace Platform Support

The crate uses a native no-replace installation primitive rather than a
hard-link or copy-and-delete emulation. Its support matrix is:

| Operation | Linux | macOS | Windows | Other targets |
| --- | --- | --- | --- | --- |
| Temp file/dir default persist (no replace) | Supported | Supported | Supported | `Unsupported` |
| Recursive copy `Fail`/`Skip` file commit | Supported | Supported | Supported | `Unsupported` |
| Temp file overwrite persist | Supported | Supported | Supported | Uses ordinary replacement support |
| Recursive copy `Overwrite` | Supported | Supported | Supported | Uses ordinary replacement support |

On an unsupported target, `temp::TempFile::persist`, no-overwrite
`temp::TempFile::persist_with`, and `temp::TempDir::persist` return
`ErrorKind::Unsupported` while retaining the temporary resource in
`temp::PersistError`. Recursive copy reports `copy::Stage::CommitFile` and
`ErrorKind::Unsupported` for `Fail` or `Skip`. It may already have created
destination directories; recursive copy does not provide transaction-wide
rollback. Overwrite operations use the ordinary replacement primitive and are
not subject to the no-replace support matrix.

## Rooted Capabilities

`rooted::Root` opens a directory descriptor and uses that descriptor as the
authority for descendant operations. Its stored absolute root path is retained
only for diagnostics. Descendant names are supplied as `rooted::Path`, and
reader, writer, and atomic-writer traversal rejects symbolic links at every
component. Renaming or replacing the root path or an intermediate name does not
redirect descriptors that were already opened.
`metadata` and `symlink_metadata` expose entry kind, size, and optional access,
modification, and creation times. Creation time is `None` when the platform's
descriptor metadata does not expose a birth time.
The operating system resolves ancestor components in the root input before the
capability is acquired; no-follow applies to the final root entry. Containment
begins after that directory descriptor has been opened.

This guarantee is descriptor-relative path containment. It does not establish
unique inode names or a complete OS security boundary: hard links, mounted
filesystems, permissions, and processes with equivalent OS authority remain
deployment concerns. The backend is available on Unix; other targets return
`ErrorKind::Unsupported` rather than falling back to check-then-path behavior.
Path-based APIs are convenience operations and are not sandbox
boundaries when another actor can mutate the namespace concurrently.

## Atomic Writes

`atomic::write` writes bytes to a temporary file in the same parent
directory, flushes and syncs that temporary file, replaces the destination, and
syncs the destination parent plus the parents of directory entries created by
the operation, from deepest to shallowest, when supported. The destination must
be absent or an existing regular file. Symbolic links, directories, FIFOs,
sockets, devices, and other special files are rejected with
`ErrorKind::InvalidInput`.

```rust
use std::io::Write;
use qubit_local_files::{
    atomic,
    temp::TempDir,
};

let dir = TempDir::with_prefix("qubit-local-files-guide-")?;
let path = dir.path().join("state").join("manifest.json");

let mut writer = atomic::begin(&path)?;
writer.write_all(br#"{"version":1,"complete":true}"#)?;
writer.commit()?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `atomic::write_with` when content generation should run inside a
guarded atomic-write callback. The callback receives `atomic::Writer`, which
supports `Write` but cannot clone or retain the underlying file handle:

```rust
use std::io::Write;

use qubit_local_files::{
    atomic,
    temp::TempDir,
};

let dir = TempDir::with_prefix("qubit-local-files-json-")?;
let path = dir.path().join("state.json");

atomic::write_with(&path, |writer| {
    writeln!(writer, "{{\"complete\":true}}")
})?;

assert_eq!("{\"complete\":true}\n", std::fs::read_to_string(&path)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `atomic::Writer` when content should be streamed across multiple calls:

```rust
use std::io::Write;
use std::time::Duration;
use qubit_local_files::atomic;

let options = atomic::Options::new()
    .with_parent()
    .with_open_retry_timeout(Duration::from_secs(5));
let mut writer =
    atomic::begin_with(std::path::Path::new("state.bin"), options)?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`atomic::Writer` implements `Write`, but not `Seek`. Only `commit` replaces
the destination. Calling `abort` or dropping the writer preserves the original
destination and cleans up the staging file. The API remains synchronous.
Use `commit_recoverable` when a caller must retry or explicitly abort after a
pre-installation failure. Its `atomic::CommitError::into_parts` returns the
structured failure and an optional retained writer; the writer is unavailable
once installation has begun. `rooted::Writer` exposes the same recovery
contract through `rooted::Root::begin_atomic_write`.

Since `0.5.0`, configuration fields are private. Callers must use the existing
getters, constructors, and builders.

On Unix, `atomic::Options::with_open_retry_timeout` limits only retries
caused by an active file lease making the nonblocking destination open return
`WouldBlock`. The default `None` waits without a deadline, while
`Duration::ZERO` returns `TimedOut` after the first conflicting attempt.
Path-based and rooted writers accept the same options before staging begins.
One-shot helpers intentionally retain the unbounded default. This setting does
not change other platforms.

### Existing-target metadata contract

Metadata preservation is strict. A failed read, ACL operation, xattr/extattr
operation, or native merge aborts the replacement instead of returning success
with weaker protection.

| Target | Metadata preserved from the current destination |
| --- | --- |
| Windows | `ReplaceFileW` with flags `0` preserves creation time, short name, object identifier, DACL, security resource attributes, encryption, compression, and named streams absent from staging. |
| Linux / Android | uid, gid, complete mode, and every descriptor-visible xattr, including exposed POSIX ACL, SELinux, and capability attributes. |
| macOS | uid, gid, mode, ACLs, and xattrs through descriptor operations and `fcopyfile`. |
| FreeBSD | uid, gid, mode, the supported POSIX or NFSv4 ACL, and user/system extattrs. |

These rows describe implemented code paths. Android and FreeBSD are
compile-only targets, so the behavior listed for them is not runtime-validated
by this repository's CI.

The crate does not clear `FILE_ATTRIBUTE_READONLY` to force replacement. A
read-only Windows destination is rejected by `ReplaceFileW` and remains
unchanged.

Unix metadata is read from the destination handle during `commit`; it is not a
snapshot taken when the writer begins. Staging metadata is synchronized before
replacement, and the destination's device/inode identity is checked again.
Unix intentionally does not promise to preserve inode or hard-link identity,
mtime/ctime, or immutable/append-only flags. Windows follows the documented
native `ReplaceFileW` merge contract.

Important semantics:

- Parent directories are created before writing.
- The temporary file is created in the destination directory, so replacement can
  be atomic on common local filesystems.
- Existing Unix metadata is captured from an opened current destination at
  commit time; Windows merges metadata inside `ReplaceFileW`.
- A destination that was absent when the writer began is installed with a
  native no-replace operation. A concurrent creator is never overwritten.
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
- Errors are reported as `atomic::Error`, which exposes the failed
  stage, temporary path, native I/O source, destination state, and any secondary
  staging cleanup error. `Unchanged`, `Replaced`, `Missing`, and `Indeterminate`
  distinguish recovery actions. Cleanup is automatic only for `Unchanged`;
  other outcomes retain any still-existing staging entry.
- The final destination inspection and replacement are separate path-based
  operations. Use `rooted::Writer` when containment must resist
  concurrent namespace replacement.
- The operation is not a multi-file transaction and does not coordinate
  concurrent writers.

## File and Directory Helpers

Focused modules provide small local filesystem helpers:

| Method | Behavior |
| --- | --- |
| `metadata::exists` | Checks whether a path exists without swallowing inspection errors. |
| `metadata::read` | Reads path metadata with `std::fs::metadata`. |
| `directory::read` | Lists direct entries of a directory. |
| `read::open` / `read::open_with` | Opens a regular file for reading; rejects directories and special resources. |
| `write::open` | Opens or creates a regular file with explicit options. |
| `directory::create_all` | Creates a directory and missing ancestors. |
| `directory::create_parent` | Creates missing parent directories for a file path. |
| `directory::size` | Sums regular-file byte lengths without following symbolic links. |
| `directory::clear` | Removes all children while keeping the directory itself. |
| `remove::any` | Removes a file, directory tree, or symbolic link. |

Example:

```rust
use std::io::Write;

use qubit_local_files::{
    directory,
    read,
    temp::TempDir,
    write,
};

let dir = TempDir::with_prefix("qubit-local-files-helpers-")?;
let path = dir.path().join("nested").join("data.txt");

let mut writer = write::open(
    &path,
    &write::OpenOptions::new(write::Mode::CreateOrTruncate).with_parents(),
)?;
writer.write_all(b"payload")?;
drop(writer);

let mut reader = read::open(&path)?;
let mut payload = String::new();
std::io::Read::read_to_string(&mut reader, &mut payload)?;
assert_eq!("payload", payload);

assert_eq!(7, directory::size(dir.path())?);
directory::clear(dir.path())?;
assert_eq!(0, directory::size(dir.path())?);

# Ok::<(), std::io::Error>(())
```

`directory::size` and `directory::clear` require the root path to be a
directory. Symbolic links are not followed. `remove::any` removes symbolic links as links, including
links that point to directories.

## Recursive Directory Copy

Use `copy::directory` when a directory tree must be copied with an
explicit conflict and symlink policy.

```rust
use qubit_local_files::{
    copy,
    directory,
    temp,
};

let dir = temp::TempDir::with_prefix("qubit-local-files-copy-")?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

directory::create_all(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = copy::directory(&src, &dst, copy::Options::default())?;

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
| `with_open_retry_timeout(...)` | unbounded | On Unix, bounds retries when a source lease conflicts with the nonblocking open; zero returns `TimedOut` after the first conflict. |

The open retry timeout is not a deadline for traversal, byte copying, commit,
or general I/O. Other platforms retain their existing behavior.

`Fail` and `Skip` file commits require the native no-replace primitive and
return `ErrorKind::Unsupported` outside Linux, macOS, and Windows. `Overwrite`
uses ordinary replacement. Destination directories created before an
unsupported file commit remain present because the operation is not a
tree-level transaction.

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
Opened source entries that are not regular files report
`std::io::ErrorKind::InvalidInput` through `copy::Error`; an unsupported
target reached through an explicitly followed symbolic link reports
`ErrorKind::Unsupported`. The structured error also exposes the failed stage, source
and destination paths, partial statistics, optional staging path, optional
secondary cleanup error, and native I/O source error. The original copy or
commit failure remains the primary source error.
The copy is not a tree-level transaction: entries committed before a failure
remain in the destination, no rollback is attempted, and destructive
type-conflict replacement may remove an existing destination directory before
a later operation fails.

Regular-file type and optional file permissions are read from the same opened
handle that supplies copied bytes. Unix uses `O_NOFOLLOW` when links are
disabled, and Windows rejects name-surrogate reparse handles. Directory
traversal, destination rechecks, and destructive replacement remain path-based,
so the policy is not an attacker-resistant sandbox when another actor can
mutate either tree concurrently.

## Filename Helpers

The `path` module contains lexical helpers that do not touch the filesystem.
Methods that return filename data return UTF-8 strings (`&str` or `String`)
instead of `OsStr`; invalid UTF-8 path components are reported as `None`.

```rust
use std::path::Path;

use qubit_local_files::path;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), path::file_stem(path));
assert_eq!(Some("archive"), path::file_prefix(path));
assert_eq!(Some("gz"), path::extension(path));
assert_eq!(Some(".gz".to_owned()), path::dot_extension(path));
assert!(path::has_extension(path, ".gz"));
assert!(path::has_extension_ignore_ascii_case(path, "GZ"));

let name = path::random_file_name_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));
assert!(name.ends_with(".tmp"));

# Ok::<(), std::io::Error>(())
```

Use `validate_portable_file_name` when a caller-provided name should be a
conservative single path component across common platforms:

```rust
use std::io::ErrorKind;

use qubit_local_files::path;

path::validate_portable_file_name("report.csv")?;

let error = path::validate_portable_file_name("CON.txt")
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
use qubit_local_files::path;

assert_eq!("file.txt", path::file_name_from_path(r"C:\tmp\file.txt"));
assert_eq!(
    "report 2026.csv",
    path::file_name_from_url("https://example.test/files/report%202026.csv?download=1"),
);
```

`file_name_from_url` excludes a syntactically valid scheme, hierarchical URL
authority, query, and fragment before selecting the last slash-delimited path
segment. Authority-only URLs return an empty string. Opaque URLs such as
`mailto:user@example.com` use their scheme-specific part as the lexical path.
The helper decodes percent-encoded UTF-8 only when the decoded value remains a
safe single filename fragment; it does not validate or normalize a complete
URL.

## Error and Cleanup Model

Simple APIs return `std::io::Result` and preserve the native error chain.
Atomic writes, recursive copy, and temporary-resource persistence use structured
errors carrying the additional state needed for safe recovery.

Important error behavior:

- `temp::PersistError` retains the temporary resource plus the persistence stage,
  requested target, resolved target when available, and native error.
- `atomic::Error::destination_state()` reports `Unchanged`, `Replaced`,
  `Missing`, or `Indeterminate`; callers must inspect both paths for an
  indeterminate result.
- Existing temporary-file persistence targets are rejected unless
  `temp::PersistOptions::new().with_overwrite()` is explicit.
- Existing temporary-directory persistence targets are rejected.
- Recursive copy uses an explicit `copy::ConflictPolicy` for existing files
  and a separate `copy::TypeConflictPolicy` for file/directory mismatches.
- Recursive copy rejects symbolic links unless
  `copy::Options::new().follow_symlinks()` is explicit.
- Drop-time cleanup failures are logged through `log::warn!` and never panic.
- `temp::TempFile::as_file`, `as_file_mut`, `Write`, and `Seek` return
  `ErrorKind::NotFound` after `close`.
- `temp::TempDir` child APIs return `ErrorKind::InvalidInput` for unsafe child
  paths, non-file child readers, and child paths that escape the temporary
  directory through symbolic links.

## Path Lengths and Platform Limits

`temp::TempFile` and `temp::TempDir` create local filesystem entries and return
operating system errors when creation fails. They do not promise that the
resulting path is valid for every platform API. Some APIs, such as Unix domain
sockets, have much shorter path limits than regular files. For those cases,
create temporary entries under a short parent directory such as `/tmp`.

Relative inputs used by temporary resources and atomic writers are bound to the
process current directory when the resource or operation begins. Temporary
resource `path`, child-path, `keep`, and persistence methods return absolute
paths that remain directly usable after later current-directory changes. The
relative source and destination paths of recursive copy are likewise bound when
copy begins, so later current-directory changes do not redirect traversal,
staging, or commit. The crate rejects interior UTF-16 NULs on Windows but does
not add a verbatim-path
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

The support tiers are explicit:

| Support tier | Targets | CI validation |
| --- | --- | --- |
| Native runtime-tested | Linux, Windows, macOS | Tests execute on the named operating system, including platform-specific filesystem behavior. |
| Compile-only | FreeBSD, Android | CI cross-compiles production and cfg-selected sources with `cargo check`; runtime filesystem, ABI, and metadata behavior are not validated or guaranteed by this repository. |

Useful commands:

```bash
cargo test
./coverage.sh
./align-ci.sh
./ci-check.sh
```
