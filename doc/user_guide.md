# Qubit Local Files User Guide

Qubit Local Files is the concrete native filesystem layer used by applications
and local-provider adapters. It does not define provider paths, registries, or
remote filesystem behavior.

## API Model

The public API has two authorities:

- `LocalFileSystem` contains host-wide associated methods.
- `RootedLocalFileSystem` is a stateful authority anchored to an opened
  directory descriptor or handle.

`LocalFileNames` and `LocalPaths` contain native lexical helpers. Readers,
writers, walkers, and temporary resources are stateful values. There are no
public free-function aliases for filesystem operations.

## Host Operations

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalCreateDirectoryOptions,
    LocalFileSystem,
    LocalReadOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
};

let root = std::path::Path::new("build/output");
LocalFileSystem::create_directory(
    root,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;

let path = root.join("manifest.json");
let mut writer =
    LocalFileSystem::open_writer(
        &path,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(LocalWriterState::Committed, result.state());

let mut reader =
    LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?;
let mut text = String::new();
reader.read_to_string(&mut text)?;

# Ok::<(), Box<dyn std::error::Error>>(())
```

Relative paths used by multi-call operations are bound at operation start.
Copy and rename bind source and target using one current-directory snapshot.
Metadata observes the final entry without following a final symbolic link.

## Copy

`LocalFileSystem::copy` and `RootedLocalFileSystem::copy` select file or
directory behavior from source metadata.

```rust
use qubit_local_files::{LocalCopyOptions, LocalFileSystem};

let outcome = LocalFileSystem::copy(
    std::path::Path::new("source"),
    std::path::Path::new("backup"),
    &LocalCopyOptions::new(),
)?;
assert!(outcome.stats().files() + outcome.stats().directories() > 0);

# Ok::<(), Box<dyn std::error::Error>>(())
```

Options separately describe target conflict, type conflict, metadata,
symbolic-link, device-boundary, recursion, atomicity, and durability policies.
Required guarantees are rejected before destructive changes when unsupported.
Copy rejects self-copy and hard-link aliases. Overwrite replaces a target
symbolic-link entry rather than following it.

## Lazy Walking

```rust
use qubit_local_files::{LocalFileSystem, LocalListOptions};

let walker = LocalFileSystem::list(
    std::path::Path::new("workspace"),
    &LocalListOptions::new().with_max_depth(2),
)?;
for entry in walker {
    let entry = entry?;
    println!("{}", entry.path().display());
}

# Ok::<(), Box<dyn std::error::Error>>(())
```

The walker opens and advances directories on demand. Depth and symbolic-link
policies are fixed when the walker is created. Dropping a walker only releases
handles.

## Writer Lifecycle

`LocalWriteMode::CreateNew` and `CreateOrReplace` use same-directory staging.
`Append` modifies an existing regular file directly and rejects required
atomicity.

A writer starts in `Open`. `commit` returns `LocalWriteOutcome`; `abort`
discards unpublished staging. A stream write or flush failure makes the state
indeterminate because an ordinary I/O error cannot prove that no bytes changed.
Commit failures use `LocalFileCommitError` to preserve publication state.

## Temporary Resources

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};

let directory = LocalFileSystem::create_temp_directory(
    &LocalTempDirectoryOptions::new().with_suffix(".work"),
)?;
let mut file = LocalFileSystem::create_temp_file(
    &LocalTempFileOptions::new()
        .with_parent(directory.path())
        .with_suffix(".data"),
)?;
file.write_all(b"payload")?;
file.close();

# Ok::<(), Box<dyn std::error::Error>>(())
```

Temporary entries own cleanup responsibility. Drop performs best-effort cleanup
while ownership remains armed. `keep` disables cleanup and returns the stable
absolute path. Persistence errors retain the resource so callers can retry,
inspect, keep, or explicitly clean it.

Prefix and suffix validation happens before entry creation. Native separators,
NUL, and portable reserved-name violations are rejected without leaving an
entry.

## Rooted Authority

```rust
use qubit_local_files::{
    LocalListOptions,
    RootedLocalFileSystem,
};

let root =
    RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let walker = root.list(
    std::path::Path::new("assets"),
    &LocalListOptions::new(),
)?;
for entry in walker {
    println!("{}", entry?.path().display());
}

# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted paths are relative descendants. Absolute paths, prefixes, `.`, and `..`
are rejected. Intermediate symbolic links are not accepted. The diagnostic
root path is non-authoritative: renaming that path after open does not redirect
the opened authority.

## Names and Paths

Filename accessors return `OsStr` or `OsString`, preserving non-UTF-8 Unix
names and native Windows values.

```rust
use qubit_local_files::{LocalFileNames, LocalPaths};

let name = LocalFileNames::random_name_with(
    Some("upload-"),
    Some(".tmp"),
)?;
LocalFileNames::validate_portable(name.as_os_str())?;

let child = LocalPaths::compose_descendant(
    std::path::Path::new("workspace"),
    std::path::Path::new(name.as_os_str()),
)?;
assert!(LocalPaths::is_lexically_within(
    &child,
    std::path::Path::new("workspace"),
)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

`bind_host_paths` should be used for related paths so one current-directory
snapshot defines the operation. Lexical containment is an early classification,
not a replacement for descriptor-relative authorization.

## Errors and Capabilities

`LocalFileError` reports `LocalFileErrorKind`, `LocalFileOperation`, primary and
target native paths, and an optional `std::io::Error` source. Publication
sessions use dedicated failure types for partial-success state.

`LocalFileSystem::capabilities()` reports the host implementation.
`RootedLocalFileSystem::capabilities()` returns the snapshot cached when the
authority was opened. Path limits carry an explicit unit: bytes on Unix or
UTF-16 code units on Windows.

## Validation

Run:

```bash
cargo test --all-features
./align-ci.sh
./ci-check.sh
```

Linux, Windows, and macOS behavior is runtime-tested. FreeBSD and Android are
compile-checked and are not documented as runtime guarantees.
