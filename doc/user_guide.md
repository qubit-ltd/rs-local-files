# Qubit Local Files User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) ·
[API reference](https://docs.rs/qubit-local-files)

This guide covers `qubit-local-files` 0.8 on Rust 1.94 or newer. It is for
applications that operate on the host filesystem or need operations restricted
to one opened directory. It is not a provider registry, a remote filesystem
API, or a replacement for provider-level logical paths.

## Conceptual Model

```
host paths ── LocalFileSystem ── native filesystem
opened root ─ RootedLocalFileSystem ─ relative descendants only
```

`LocalFileSystem` exposes host-wide associated methods. `RootedLocalFileSystem`
is a stateful authority created with `open`; it keeps the opened root rather
than repeatedly resolving a string path. Readers, writers, walkers, and
temporary entries are owned stateful resources. `LocalFileNames` and
`LocalPaths` provide native lexical utilities without converting names to UTF-8.

## Scenario: write and inspect an export

An exporter must create `build/output`, publish `manifest.json` only after a
complete write, and inspect the result. The observable success condition is a
`Committed` writer outcome and the bytes read back from the published file.

```rust
use std::io::{Read, Write};
use qubit_local_files::{
    LocalCreateDirectoryOptions, LocalFileSystem, LocalReadOptions,
    LocalWriteMode, LocalWriteOptions, LocalWriterState,
};

let output = std::path::Path::new("build/output");
LocalFileSystem::create_directory(
    output,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;
let path = output.join("manifest.json");
let mut writer = LocalFileSystem::open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?
    .read_to_string(&mut text)?;
assert_eq!(text, r#"{"complete":true}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Relative paths used by a multi-call operation are bound when that operation
starts. Copy and rename bind source and target from one current-directory
snapshot. `metadata` observes the final entry without following a final
symbolic link.

## Publish, Copy, and Recover

`CreateNew` and `CreateOrReplace` use same-directory staging. `Append` changes
an existing regular file directly and rejects required atomicity. A writer can
be committed or aborted; lifecycle (`LocalWriterState`) is separate from
publication conclusion (`LocalWriteFailureState`). A write, flush, or commit
error can leave publication state indeterminate, so retain and inspect the
returned resource/error where recovery is required.

`copy` selects file or directory behavior from source metadata. Use
`with_file_source()` or `with_tree_source()` when the source kind must be
explicit; `source_mode()` reports the selected mode. Its options separately
control target conflict, type conflict, metadata, symbolic links, device
boundaries, atomicity, and durability. Unsupported required
guarantees are rejected before destructive changes. Self-copy and hard-link
aliases are rejected; overwriting a symbolic-link target replaces that entry
rather than following it.

```rust,no_run
use qubit_local_files::{LocalCopyFailureState, LocalCopyOptions, LocalFileSystem};

match LocalFileSystem::copy(
    std::path::Path::new("source"),
    std::path::Path::new("backup"),
    &LocalCopyOptions::new(),
) {
    Ok(outcome) => println!("copied {} files", outcome.stats().files()),
    Err(failure) => match failure.state() {
        LocalCopyFailureState::Unchanged => println!("destination is unchanged"),
        LocalCopyFailureState::PartiallyPublished => println!("destination is partial"),
        LocalCopyFailureState::Published => println!("destination was published"),
        LocalCopyFailureState::Indeterminate => println!("reconcile destination"),
    },
}
```

Rename reports `Unchanged`, `Renamed`, or `Indeterminate` through its typed
failure state for the same reason: an error is not necessarily “nothing
happened”.

## Walk and Temporary Resources

`LocalFileSystem::list` returns a lazy `LocalDirectoryWalker`. It opens and
advances directories on demand; maximum depth and symbolic-link policy are
fixed at creation, and dropping it only releases handles.

Temporary files and directories own cleanup while armed. Dropping them performs
best-effort cleanup; `keep` disables cleanup and returns a stable absolute path.
Persistence failures retain the resource so the caller can retry, inspect,
keep, or explicitly clean it. Prefixes and suffixes are checked before entry
creation: native separators, NUL, and portable reserved-name violations do not
leave an entry behind.

## Rooted Workspaces

Use rooted access when processing untrusted relative names beneath a workspace.

```rust
use qubit_local_files::{LocalListOptions, RootedLocalFileSystem};

let root = RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let walker = root.list(std::path::Path::new("assets"), &LocalListOptions::new())?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted paths must be relative descendants. Absolute paths, prefixes, `.`, `..`,
and intermediate symbolic links are rejected. The diagnostic root path is not
the authority: renaming it after `open` does not redirect the resource.
Lexical containment is useful early classification, but it is not a substitute
for descriptor-relative authorization.

## Errors, Diagnostics, and Troubleshooting

`LocalFileError` carries a `LocalFileErrorKind`, a `LocalFileOperation`, native
primary and target paths when available, and an optional `std::io::Error`
source. Publication operations use dedicated failure types to preserve
partial-success state.

`LocalPersistError` retains the temporary resource and its structured
`LocalFileError`; its `state()` is the single recovery-state authority. Native
I/O errors are available through the structured error source when present.

| Symptom | Check |
| --- | --- |
| A rooted operation rejects a path | Pass a relative descendant; remove absolute prefixes, `.`, `..`, and intermediate symlinks. |
| A required guarantee is rejected | Inspect the selected filesystem capabilities and relax the requirement only if the application permits it. |
| Copy or rename returns an error | Inspect its typed failure state before retrying, cleanup, or treating the target as absent. |
| A temporary entry remains | Retain the resource and call its explicit lifecycle method; drop cleanup is best effort. |

## Platform Limits and Further Reading

Linux, Windows, and macOS are runtime-tested. FreeBSD and Android are
compile-checked only. `LocalFileSystem::capabilities()` reports the host
implementation; `RootedLocalFileSystem::capabilities()` is the snapshot cached
when opening the authority. A path limit is `Some` only when verified for the
target filesystem. Atomic rename, atomic replacement, and atomic temporary
persistence are reported independently because platform support differs.

Continue with the [README](../README.md), [中文用户手册](user_guide.zh_CN.md),
or the [API reference](https://docs.rs/qubit-local-files).
