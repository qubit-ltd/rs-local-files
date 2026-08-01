# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

`qubit-local-files` is a policy-aware native filesystem API for applications
that need more than ad-hoc `std::fs` calls: structured error context, explicit
publication outcomes, lazy traversal, temporary-resource ownership, and an
authority rooted in an opened directory. It works directly with native `Path`
and `OsStr` values and does not depend on `qubit-fs`; provider adaptation lives
in [`qubit-fs-local`](https://crates.io/crates/qubit-fs-local).

## Installation

```toml
[dependencies]
qubit-local-files = "0.8"
```

## Quick Start: publish a generated file

Build tools and exporters often need to replace an output only after all bytes
have been written. This example creates a work directory, writes a manifest
through a writer, commits it, and reads the published result.

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalFileSystem, LocalReadOptions, LocalTempDirectoryOptions, LocalWriteMode,
    LocalWriteOptions, LocalWriterState,
};

let work = LocalFileSystem::create_temp_directory(&LocalTempDirectoryOptions::new())?;
let path = work.path().join("manifest.json");
let mut writer = LocalFileSystem::open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(outcome.state(), LocalWriterState::Committed);

let mut content = String::new();
LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?
    .read_to_string(&mut content)?;
assert_eq!(content, r#"{"version":1}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## What It Provides

| API | Use it when you need |
| --- | --- |
| `LocalFileSystem` | Host-wide metadata, I/O, copy, rename, traversal, and temporary entries. |
| `RootedLocalFileSystem` | Access beneath one opened directory authority. |
| `LocalFileWriter` | Explicit commit or abort after staged publication. |
| `LocalDirectoryWalker` | Lazy directory enumeration with fixed creation-time policy. |
| `LocalTempFile` / `LocalTempDirectory` | Owned cleanup, `keep`, and persistence operations. |
| `LocalFileNames` / `LocalPaths` | Native filename and lexical-path helpers without lossy UTF-8 conversion. |

All filesystem operations are associated methods or methods on stateful
resources; the crate exposes no legacy free-function namespaces.

Temporary-resource cleanup is ownership-aware, not a synchronization boundary.
Before deleting, a guard checks that the path still has the identity captured
at creation, so ordinary replacement is rejected. The identity check and path
deletion are separate operating-system operations, however. If an untrusted
actor can mutate the same directory concurrently, it can race those operations,
and a filesystem may eventually reuse an identity. For example, removing a
temporary file and repeatedly installing another file at the same name is
outside the cleanup guarantee. Put temporary entries in a directory not
writable by concurrent actors, or call `keep` and coordinate deletion yourself.

## Choose the right authority

Use `LocalFileSystem` for host paths. Use `RootedLocalFileSystem` when one
opened directory is the authority boundary: every operational path is a
relative descendant, and absolute paths, prefixes, `.`, `..`, and intermediate
symbolic links are rejected. Renaming the diagnostic root path later does not
redirect the opened authority.

Copy chooses file or directory behavior from source metadata. Copy and rename
failures retain the strongest proven publication state, so callers must inspect
the typed failure instead of assuming that an error leaves the destination
unchanged. `CreateNew` and `CreateOrReplace` stage in the destination directory;
`Append` writes an existing regular file directly and cannot satisfy required
atomicity.

## Learn More

- [User guide](doc/user_guide.md)
- [用户手册](doc/user_guide.zh_CN.md)
- [API reference](https://docs.rs/qubit-local-files)
- [中文设计文档](doc/local_file_system_design.zh_CN.md)
- [中文 README](README.zh_CN.md)

## Platform Scope

Linux, Windows, and macOS behavior is runtime-tested. FreeBSD and Android
configuration paths are compile-checked only; this crate makes no runtime
guarantee for those targets. Capability snapshots report mechanisms implemented
for the selected build; they do not probe a particular runtime filesystem.
Required atomicity or durability is
rejected before namespace changes when it cannot be met.

## Testing

```bash
# Run tests with the default feature set
cargo test

# Run tests with all declared features
cargo test --all-features

# Project CI checks
./ci-check.sh

# Check code coverage
./coverage.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Please follow the Rust API guidelines, keep public
API documentation and tests current, and run `./align-ci.sh` to format code and
`./ci-check.sh` to satisfy CI requirements before submitting a pull request.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
