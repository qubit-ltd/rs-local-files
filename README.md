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
qubit-local-files = "0.3"
```

## Quick Start: publish a generated file

Build tools and exporters often need to replace an output only after all bytes
have been written. This example creates a work directory, writes a manifest
through a writer, commits it, and reads the published result.

```rust
use std::io::{Read, Write};

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{LocalWriteMode, LocalWriteOptions};
use qubit_local_files::outcome::LocalWriterState;

let mut filesystem = LocalFileSystem::host()?;
filesystem.set_default_write_options(LocalWriteOptions::new(
    LocalWriteMode::CreateOrReplace,
))?;

let work = filesystem.create_temp_directory()?;
let path = work.path().join("manifest.json");
let mut writer = filesystem.open_writer(&path)?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(outcome.state(), LocalWriterState::Committed);

let mut content = String::new();
filesystem.open_reader(&path)?
    .read_to_string(&mut content)?;
assert_eq!(content, r#"{"version":1}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Why This Project Exists

`std::fs` is the right choice for isolated file operations. Applications that
also need a stable authority boundary, reusable operation policy, bounded
traversal, staged publication, or recovery after partial success otherwise
have to design those contracts themselves. `qubit-local-files` keeps those
decisions in one stateful filesystem object and reports outcomes explicitly.

The crate remains synchronous and local: it does not provide remote storage,
provider registration, application authorization, or async I/O.

## What It Provides

| API | Use it when you need |
| --- | --- |
| `LocalFileSystem::host()` | A reusable service over the process-visible host namespace. |
| `LocalFileSystem::rooted(root)` | The same operations beneath one opened directory authority. |
| `path::LocalFileSystemScope` | Whether an instance interprets paths as host paths or rooted descendants. |
| `LocalFileWriter` | Explicit commit or abort after staged publication. |
| `LocalDirectoryWalker` | Lazy directory enumeration with fixed creation-time policy. |
| `LocalTempFile` / `LocalTempDirectory` | Owned cleanup, `keep`, and persistence operations. |
| `path::LocalFileNames` / `path::LocalPaths` | Native filename and lexical-path helpers without lossy UTF-8 conversion. |

Readers and writers accept regular files only; directories and special files
are rejected instead of being opened as byte streams.

`LocalFileSystem` is a stateful instance API. Rooted instances own a virtual
current directory; Host instances read the process current directory only when
an operation needs to bind a relative path. Each instance also owns its
symbolic-link policy and nine operation-default Options values.
Ordinary methods use those defaults; every `*_with_options` method uses the
supplied complete Options value instead. Cloning snapshots all mutable state,
while Rooted clones share only the immutable opened authority. The crate makes
no thread-safety promise for shared mutable configuration; callers may keep one
clone per thread or add their own synchronization. Calling
`set_current_directory` on a Host instance changes the process-global current
directory; calling it on a Rooted instance changes only that instance.

Resource budgets are opt-in. Listing and copy depth, entries, bytes, open
directories, deadlines, duplicate-name memory, retry timeouts, and temporary
name attempts are unbounded or disabled until the caller sets them explicitly.

Symbolic-link behavior is configurable per `LocalFileSystem` instance; Rooted
defaults to `FollowWithinScope` and Host defaults to `FollowAcrossScope`. Rooted
supports only `Reject` and `FollowWithinScope`; selecting `FollowAcrossScope`
returns `InvalidOptions`. See the [user guide](doc/user_guide.md) for
operation-specific final-link semantics.

Temporary-resource cleanup is ownership-aware, not a synchronization boundary.
Call `cleanup()` when the caller must observe cleanup failures; dropping a
resource is a silent best-effort fallback and never reports or logs failure.
Before deleting, a guard checks that the path still has the identity captured
at creation, so ordinary replacement is rejected. The identity check and path
deletion are separate operating-system operations, however. If an untrusted
actor can mutate the same directory concurrently, it can race those operations,
and a filesystem may eventually reuse an identity. For example, removing a
temporary file and repeatedly installing another file at the same name is
outside the cleanup guarantee. Put temporary entries in a directory not
writable by concurrent actors, or call `keep` and coordinate deletion yourself.
Each temporary resource is created inside a private per-resource sandbox. The
resource path therefore includes one generated sandbox component; `keep`
atomically publishes the entry to a generated sibling path, returns a
`LocalPersistOutcome`, and reports whether sandbox cleanup left a residual.

## Choose the right authority

Use `LocalFileSystem::host()` for host paths. Use
`LocalFileSystem::rooted(root)` when one opened directory is the authority
boundary. Both instances expose the same operations; only path interpretation
changes. Host absolute paths never query the process PWD. A Host relative path
captures one process-PWD snapshot when the operation begins; a Rooted relative
path uses the instance's virtual PWD. `.` and an empty path mean the applicable
PWD; `..` is normalized one component at a time and is rejected only if it
would cross the namespace root. In a Rooted instance, `/etc/hosts` is a virtual
absolute path beneath the opened root, not the Host path `/etc/hosts`. Native
prefixes are rejected.

Intermediate symbolic links follow the configured policy. Rooted absolute link
targets restart at its virtual `/`, and `FollowWithinScope` prevents any link
from escaping the opened authority. `FollowAcrossScope` is Host-only; Rooted
rejects that configuration. Renaming the diagnostic root path later does not
redirect the opened authority. Public resource and error paths use reusable
namespace-absolute identities; physical paths, when available, are exposed
only as optional diagnostics.
On Windows, Rooted link inspection, link-kind detection, and link creation are
all relative to opened handles. Copying a dangling link, or a link whose target
is outside the Rooted authority, does not inspect or open that target.

Copy chooses file or directory behavior from source metadata. Copy and rename
failures retain the strongest proven publication state, so callers must inspect
the typed failure instead of assuming that an error leaves the destination
unchanged. `CreateNew` and `CreateOrReplace` stage in the destination directory;
`Append` writes an existing regular file directly and cannot satisfy required
atomicity.

## Platform Scope

Linux, Windows, and macOS behavior is runtime-tested. FreeBSD and Android
configuration paths are compile-checked only; this crate makes no runtime
guarantee for those targets. `capabilities()` reports whether this build
implements each complete operation protocol; it does not probe a particular
runtime filesystem or claim that the underlying hardware has persisted data.
Required atomicity or durability is rejected before namespace changes when the
protocol cannot be met.
Atomic rename, atomic replacement, the ability to attempt atomic temporary
persistence, durable rename, durable file copy, and durable writer publication
are reported as independent capabilities because their platform support
differs. `can_attempt_atomic_temp_persist()` is an implementation capability,
not a promise for arbitrary source and target paths; the operation outcome
reports what actually happened.
Path-limit observations always include a unit: Unix reports bytes and Windows
reports UTF-16 code units. Windows whole-path limits remain `Unknown` when the
handle-relative namespace has no defensible fixed bound. `space_at()` and
component limits are queried from the selected filesystem handle. Windows Host
path conversion intentionally does not support UNC paths.

## Learn More

- [User guide](doc/user_guide.md)
- [用户手册](doc/user_guide.zh_CN.md)
- [API reference](https://docs.rs/qubit-local-files)
- [Design document](doc/local_file_system_design.md)
- [中文设计文档](doc/local_file_system_design.zh_CN.md)
- [中文 README](README.zh_CN.md)

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
