# Qubit Local Files User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) ·
[API reference](https://docs.rs/qubit-local-files)

This guide covers `qubit-local-files` 0.3 on Rust 1.94 or newer. It is for
applications that operate on the host filesystem or need operations restricted
to one opened directory. It is not a provider registry, a remote filesystem
API, or a replacement for provider-level logical paths.

## Conceptual Model

```
host paths ── LocalFileSystem::host()
opened root ─ LocalFileSystem::rooted(root) ─ relative descendants only
```

`LocalFileSystem` is the host-wide service form: `host()` selects
process-visible paths, while
`rooted(root)` opens a directory authority and accepts only relative
descendants. The two forms expose the same operations, so callers and adapters
do not need separate host and rooted interfaces. Readers, writers, walkers, and
temporary entries are owned stateful resources. `LocalFileNames` and
`LocalPaths` provide native lexical utilities without converting names to UTF-8.

## Symbolic-link policy

`LocalFileSystem` stores one symbolic-link policy inherited by all operations.
`LocalFileSystem::rooted(root)` defaults to `FollowWithinScope`; it follows
links only while the resolved path remains below the opened root. Host defaults
to `FollowAcrossScope`, because Host has no narrower root boundary. Rooted
supports only `Reject` and `FollowWithinScope`; configuring
`FollowAcrossScope` returns `InvalidOptions`. The fallible
`with_symlink_policy` method and list/copy options can select a supported policy.

The policy applies to every non-final path component. With
`FollowWithinScope`, a rooted path such as `etc/link/config` is rejected when
`link` resolves outside the opened directory. `FollowAcrossScope` is available
only in Host mode.

Final components retain native operation semantics:

| Operation | Final symbolic link |
| --- | --- |
| `metadata` | Inspects the link entry itself. |
| `open_reader` | Follows the link on Unix; rejects a final name-surrogate reparse point on Windows. |
| `CreateNew` writer | Treats an existing link as an existing entry. |
| `Append` writer | Follows the link and appends to its target. |
| `CreateOrReplace` writer | Follows the link, replaces its target, and preserves the link. |
| `delete` | Removes the link entry. |
| `rename` | Moves or replaces the link entry. |
| `copy` source | Copies the link entry itself. |
| `copy` target | Replaces the target link entry. |
| `temp persist` | Publishes by rename and replaces the target link entry. |

Listing follows directory links when the effective policy allows it. Returned
paths remain logical paths through the link (for example, `link/child`), and
recursive traversal detects directory-identity cycles. Depth counts logical
entries; crossing a link does not add another level.

## Scenario: write and inspect an export

An exporter must create `build/output`, publish `manifest.json` only after a
complete write, and inspect the result. The observable success condition is a
`Committed` writer outcome and the bytes read back from the published file.

```rust
use std::io::{Read, Write};
use qubit_local_files::{
    LocalCreateDirectoryOptions, LocalFileSystem, LocalReadOptions, LocalWriteMode,
    LocalWriteOptions, LocalWriterState,
};

let filesystem = LocalFileSystem::host();
let output = std::path::Path::new("build/output");
filesystem.create_directory(
    output,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;
let path = output.join("manifest.json");
let mut writer = filesystem.open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
filesystem.open_reader(&path, &LocalReadOptions::new())?
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

`LocalFileSystem::copy` selects file or directory behavior from source metadata. Use
`with_file_source()` or `with_tree_source()` when the source kind must be
explicit; `source_mode()` reports the selected mode. Its options separately
control target conflict, type conflict, metadata, symbolic links, atomicity,
and durability. Mount and device boundaries are not part of the copy policy.
Unsupported required guarantees are rejected before destructive changes. Self-copy and hard-link
aliases are rejected; overwriting a symbolic-link target replaces that entry
rather than following it.

```rust,no_run
use qubit_local_files::{LocalCopyFailureState, LocalCopyOptions, LocalFileSystem};

match LocalFileSystem::host().copy(
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
advances directories on demand; maximum depth, symbolic-link policy, and the
handle budget are fixed at creation. The default `Reopen` policy closes and
reopens active frames when the budget is reached, while `Fail` explicitly
returns `ResourceLimit`; a zero handle budget is invalid and returns
`InvalidOptions`. Rooted enumeration also streams each directory instead of
first collecting it into a vector. Dropping it only releases handles.

Temporary files and directories own cleanup while armed. Each resource lives in
a private generated sandbox that is removed with the resource. Dropping them
performs best-effort cleanup; `keep` disables cleanup and returns an authority-local path
(absolute for Host and relative to the opened root for Rooted).
Persistence failures retain the resource so the caller can retry, inspect,
keep, or explicitly clean it. Prefixes and suffixes are checked before entry
creation: native separators, NUL, and portable reserved-name violations do not
leave an entry behind.

## Rooted Workspaces

Use rooted access when processing untrusted relative names beneath a workspace.

```rust
use qubit_local_files::{LocalFileSystem, LocalListOptions};

let root = LocalFileSystem::rooted(std::path::Path::new("workspace"))?;
let walker = root.list(std::path::Path::new("assets"), &LocalListOptions::new())?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted paths must be relative descendants. Absolute paths, prefixes, `.`, and
`..` are rejected. Intermediate symbolic links follow the configured policy;
`FollowWithinScope` rejects a link that resolves outside the root. The
Rooted namespace does not support `FollowAcrossScope`; that configuration
returns `InvalidOptions`. The diagnostic root path is not the authority for
descriptor-relative operations: renaming it after `open` does not redirect
those operations.
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
| A rooted operation rejects a path | Pass a relative descendant and remove absolute prefixes, `.`, and `..`; an escaping intermediate link returns `InvalidPath`, while selecting `FollowAcrossScope` returns `InvalidOptions`. |
| A required guarantee is rejected | Inspect the selected filesystem capabilities and relax the requirement only if the application permits it. |
| Copy or rename returns an error | Inspect its typed failure state before retrying, cleanup, or treating the target as absent. |
| A temporary entry remains | Retain the resource and call its explicit lifecycle method; drop cleanup is best effort. |

## Platform Limits and Further Reading

Linux, Windows, and macOS are runtime-tested. FreeBSD and Android are
compile-checked only. `LocalFileSystem::host().protocols()` reports the host
implementation; a rooted instance returns the snapshot cached when opening the
authority. `scope()` lets integration code distinguish the two namespaces, and
`diagnostic_root()` exposes the non-authoritative rooted anchor separately. A
`limits()` reports `SizeLimit::VariesByPath` for the host namespace; use
`limits_at(path)` to obtain a finite value for the filesystem containing that
path (or `Unknown` when probing is unavailable). Atomic
rename, atomic replacement, and atomic temporary persistence are reported
independently because platform support differs.

Continue with the [README](../README.md), [中文用户手册](user_guide.zh_CN.md),
or the [API reference](https://docs.rs/qubit-local-files).
