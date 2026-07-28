# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![中文文档](https://img.shields.io/badge/文档-中文版-blue.svg)](README.zh_CN.md)

Unified native local filesystem operations for Rust.

## Overview

Qubit Local Files provides one policy-driven API for host-wide and
descriptor-anchored local filesystem operations. It accepts native `Path` and
`OsStr` values, preserves platform filenames, and keeps platform-specific
containment and publication logic inside this crate.

Use it for:

- structured local filesystem errors and outcomes;
- unified file and directory copy;
- lazy recursive walking;
- staged file publication and explicit append semantics;
- RAII temporary files and directories;
- descriptor- or handle-relative rooted access;
- native path and filename validation.

The crate does not depend on `qubit-fs`. Provider-neutral adaptation belongs in
`qubit-fs-local`.

See the [User Guide](doc/user_guide.md) and the
[design document](doc/local_file_system_design.zh_CN.md) for the complete
contract.

## Installation

```toml
[dependencies]
qubit-local-files = "0.7"
```

## Quick Example

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalFileSystem,
    LocalReadOptions,
    LocalTempDirectoryOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
};

let work =
    LocalFileSystem::create_temp_directory(&LocalTempDirectoryOptions::new())?;
let path = work.path().join("state.json");

let mut writer =
    LocalFileSystem::open_writer(
        &path,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(LocalWriterState::Committed, outcome.state());

let mut reader =
    LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?;
let mut content = String::new();
reader.read_to_string(&mut content)?;
assert_eq!(r#"{"version":1}"#, content);

# Ok::<(), Box<dyn std::error::Error>>(())
```

## Main Capabilities

| Type | Purpose |
| --- | --- |
| `LocalFileSystem` | Host metadata, read, write, copy, walk, create, delete, rename, and temporary resources. |
| `RootedLocalFileSystem` | Stateful descriptor- or handle-relative authority beneath one opened root. |
| `LocalFileNames` / `LocalPaths` | Native lexical utilities without lossy UTF-8 conversion. |
| `LocalDirectoryWalker` | Lazy traversal with fixed depth and symlink policy. |
| `LocalFileReader` / `LocalFileWriter` | Owned I/O resources with explicit publication state. |
| `LocalTempFile` / `LocalTempDirectory` | Cleanup-owned temporary entries. |
| `LocalFileError` | Stable classification plus operation and native path context. |

All operations use associated methods or stateful resource methods. Legacy
free-function namespaces are not part of the public API.

## Rooted Access

```rust
use std::io::Read;

use qubit_local_files::{
    LocalReadOptions,
    RootedLocalFileSystem,
};

let root = RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let mut reader =
    root.open_reader(std::path::Path::new("config/app.toml"), &LocalReadOptions::new())?;
let mut content = String::new();
reader.read_to_string(&mut content)?;

# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted operations reject absolute paths, platform prefixes, `.` and `..`, and
derive descendant access from the opened root authority rather than a later
string lookup of its diagnostic path.

## Platform Support

| Support tier | Targets | Validation |
| --- | --- | --- |
| Runtime-tested | Linux, Windows, macOS | Platform filesystem tests run in CI. |
| Compile-only | FreeBSD, Android | Production cfg paths are checked; runtime guarantees are not claimed. |

Capability snapshots report guarantees that the selected implementation can
provide. Required atomicity or durability is rejected before namespace changes
when it cannot be satisfied.

## Runtime Dependencies

The crate uses the Rust standard library, `getrandom`, `libc`, `log`, and
target-specific `windows-sys` bindings.

## Testing

```bash
cargo test --all-features
./align-ci.sh
./ci-check.sh
```

## License

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for the
full license text.

## Contributing

Contributions are welcome. Keep public documentation and platform behavior
tests current, then run `./align-ci.sh` and `./ci-check.sh`.

## Author

**Haixing Hu** - *Qubit Co. Ltd.*

Repository: [https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
