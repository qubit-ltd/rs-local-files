# Qubit Local Files User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) ·
[API reference](https://docs.rs/qubit-local-files)

This guide covers `qubit-local-files` 0.3 on Rust 1.94 or newer. It is for
applications that operate on the host filesystem or need operations restricted
to one opened directory. It is not a provider registry, a remote filesystem
API, or a replacement for provider-level logical paths.

## Conceptual Model

```
Host namespace ── LocalFileSystem::host() ── operation-time process PWD
opened root ───── LocalFileSystem::rooted(root) ── virtual / and instance PWD
```

`LocalFileSystem` is a stateful filesystem object. `host()` selects the
process-visible namespace without reading the current directory. An absolute
Host path never requires it; a relative Host path captures one process-PWD
snapshot when its operation begins.
`rooted(root)` opens one directory authority, gives it the virtual root `/`,
and starts with PWD `/`. Both forms accept namespace-absolute paths and paths
relative to the applicable PWD, and expose the same operations. Readers, writers,
walkers, and temporary entries are owned stateful resources. `LocalFileNames`
and `LocalPaths` provide native lexical utilities without converting names to
UTF-8.

## Configure Once, Override Deliberately

Each Rooted instance owns a virtual PWD. Host instances instead observe the
process-global PWD. Every instance owns its symbolic-link policy and defaults for
read, write, list, copy, create-directory, delete, rename, temporary-file, and
temporary-directory operations. Configure those values once with the
`set_default_*_options` methods and then use ordinary operation methods.

Every `*_with_options` method instead uses the supplied Options as a complete
one-call replacement; it does not merge them with the instance defaults. To
modify one field from an instance default, clone or copy that default
explicitly, modify it, and pass the resulting value.

```rust,no_run
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalListOptions;

let mut filesystem = LocalFileSystem::rooted(std::path::Path::new("/srv/app"))?;
filesystem.set_current_directory(std::path::Path::new("/assets"))?;
filesystem.set_default_list_options(
    LocalListOptions::new().with_recursive().with_max_entries(10_000),
)?;

let default_walk = filesystem.list(std::path::Path::new("."))?;
let one_level = filesystem.list_with_options(
    std::path::Path::new("."),
    &LocalListOptions::new(),
)?;
# drop((default_walk, one_level));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The initial Options contain no hidden business resource caps. Traversal and
copy budgets, retry durations, deadlines, and temporary-name attempt limits
apply only when the caller sets them. Cloning snapshots a Rooted virtual PWD
and all configuration; Host clones continue to observe the same process PWD.
Rooted clones share only the immutable opened authority.
The crate does not promise synchronization for shared mutable configuration.
Use one clone per thread or add a caller-owned synchronization wrapper.

## Symbolic-link policy

`LocalFileSystem` stores one symbolic-link policy inherited by all operations.
`LocalFileSystem::rooted(root)` defaults to `FollowWithinScope`; it follows
links only while the resolved path remains below the opened root. Host defaults
to `FollowAcrossScope`, because Host has no narrower root boundary. Rooted
supports only `Reject` and `FollowWithinScope`; configuring
`FollowAcrossScope` returns `InvalidOptions`. The fallible
`set_symlink_policy` method and list/copy options can select a supported policy.

The policy applies to every non-final path component. With
`FollowWithinScope`, a rooted path such as `etc/link/config` is rejected when
`link` resolves outside the opened directory. `FollowAcrossScope` is available
only in Host mode. A Rooted link target beginning with `/` restarts at the
Rooted virtual root, not at the Host root. `.` and `..` in link targets retain
their native lexical meaning, but resolving `..` across the virtual root
returns `InvalidPath`.

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
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{
    LocalCreateDirectoryOptions, LocalWriteMode, LocalWriteOptions,
};
use qubit_local_files::outcome::LocalWriterState;

let mut filesystem = LocalFileSystem::host()?;
filesystem.set_default_create_directory_options(
    LocalCreateDirectoryOptions::new().with_recursive(),
)?;
filesystem.set_default_write_options(LocalWriteOptions::new(
    LocalWriteMode::CreateOrReplace,
))?;

let output = std::path::Path::new("build/output");
filesystem.create_directory(output)?;
let path = output.join("manifest.json");
let mut writer = filesystem.open_writer(&path)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
filesystem.open_reader(&path)?
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

`LocalFileSystem::copy` selects file or directory behavior from source
metadata. Use `with_file_source()` or `with_tree_source()` when the source
kind must be explicit; `source_mode()` reports the selected mode. Copy Options
separately control target conflict, type conflict, metadata, symbolic links,
atomicity, durability, and caller-selected resource budgets. Mount and device
boundaries are not part of the copy policy. Unsupported required guarantees
are rejected before destructive changes. Self-copy and hard-link aliases are
rejected; overwriting a symbolic-link target replaces that entry rather than
following it.

```rust,no_run
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::outcome::LocalCopyFailureState;

let filesystem = LocalFileSystem::host()?;
match filesystem.copy_with_options(
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
# Ok::<(), Box<dyn std::error::Error>>(())
```

Rename reports `Unchanged`, `Renamed`, or `Indeterminate` through its typed
failure state for the same reason: an error is not necessarily “nothing
happened”.

## Walk and Temporary Resources

`LocalFileSystem::list` returns a lazy `LocalDirectoryWalker`. It opens and
advances directories on demand; its normalized root, Options, symbolic-link
policy, PWD snapshot, and authority are fixed at creation. No depth, entry,
name-memory, deadline, or open-directory budget exists by default. When a
caller sets an open-directory budget, `Reopen` closes and later reopens active
frames as needed, while `Fail` returns `ResourceLimit` at the boundary. A
zero handle budget is invalid and returns `InvalidOptions`. Rooted enumeration
streams each directory instead of first collecting it into a vector. Dropping
the walker only releases handles.

Temporary files and directories own cleanup while armed. Each resource lives in
a private generated sandbox that is removed with the resource. Dropping them
performs silent best-effort cleanup; call `cleanup()` when the caller must observe a
cleanup failure. `keep` atomically publishes to a generated sibling outside
the sandbox and returns a `LocalPersistOutcome`; its cleanup state reports any
residual sandbox. With no explicit parent, creation
uses the filesystem PWD
captured for that operation. `path()`, `keep`, and persistence outcomes all
return namespace-absolute paths for both Host and Rooted, so they can be passed
back to the same filesystem independently of later PWD changes. Persistence
failures retain the resource so the caller can retry, inspect, keep, or
explicitly clean it. Prefixes and suffixes are checked before entry creation:
native separators, NUL, and portable reserved-name violations do not leave an
entry behind. Name-collision attempts are unbounded unless the caller sets
`max_attempts`.

## Rooted Workspaces

Use rooted access when processing untrusted relative names beneath a workspace.

```rust,no_run
use qubit_local_files::LocalFileSystem;

let mut root = LocalFileSystem::rooted(std::path::Path::new("workspace"))?;
root.set_current_directory(std::path::Path::new("/assets"))?;
let walker = root.list(std::path::Path::new("."))?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted behaves like a private namespace rooted at the opened directory:
`/etc/hosts` maps beneath that authority, while `etc/hosts` starts at the
instance PWD. `.` and an empty path mean PWD; `a/./b` normalizes to `a/b`;
`a/../b` normalizes to `b`. Parent components are accepted until one would
cross virtual `/`; therefore `..` at PWD `/` and
`a/./.././../b` at PWD `/` return `InvalidPath`. Native prefixes are
always invalid in Rooted.

Intermediate symbolic links follow the configured policy;
`FollowWithinScope` rejects a link that resolves outside the root. Rooted
does not support `FollowAcrossScope`; that configuration returns
`InvalidOptions`. The construction-time path returned by `diagnostic_root()`
is not the authority for descriptor-relative operations: renaming it after
opening does not redirect those operations. Lexical containment is useful
early classification, but it is not a substitute for handle-relative
authorization.
Windows Rooted symbolic-link reads, type checks, and creation remain relative
to opened handles. Copying the link itself never opens its dangling or external
target.

## Errors, Diagnostics, and Troubleshooting

`LocalFileError` carries a `LocalFileErrorKind`, a `LocalFileOperation`,
namespace-absolute primary and target paths when available, the operation's PWD
snapshot, and an optional typed source. Physical paths are optional diagnostics
and never define Rooted authority. Publication operations use dedicated failure
types to preserve partial-success state.

`LocalPersistError` retains the temporary resource and its structured
`LocalFileError`; its `state()` is the single recovery-state authority. Native
I/O errors are available through the structured error source when present.

| Symptom | Check |
| --- | --- |
| A Rooted operation rejects a path | Check whether lexical `..` or a followed link crosses virtual `/`, or whether the input contains a native prefix. Virtual absolute paths, `.`, and contained `..` are valid. Selecting `FollowAcrossScope` returns `InvalidOptions`. |
| A required guarantee is rejected | Inspect the selected filesystem capabilities and relax the requirement only if the application permits it. |
| Copy or rename returns an error | Inspect its typed failure state before retrying, cleanup, or treating the target as absent. |
| A temporary entry remains | Retain the resource and call its explicit lifecycle method; drop cleanup is best effort. |

## Platform Limits and Further Reading

Linux, Windows, and macOS are runtime-tested. FreeBSD and Android are
compile-checked only. `capabilities()` reports the selected authority's build
capability snapshot; a Rooted instance caches it when opening the authority. `scope()`
lets integration code distinguish the two namespaces, and
`diagnostic_root()` exposes the non-authoritative Rooted anchor separately.
`limits()` reports `SizeLimit::VariesByPath` for the Host namespace; use
`limits_at(path)` to obtain a finite value for the filesystem containing that
path (or `Unknown` when probing is unavailable). Interpret both numeric limits
using `length_unit()`: Unix uses bytes and Windows uses UTF-16 code units, which
must not be treated as UTF-8 byte limits. Atomic
rename, atomic replacement, and atomic temporary persistence are reported
independently because platform support differs.

Continue with the [README](../README.md), [中文用户手册](user_guide.zh_CN.md),
or the [API reference](https://docs.rs/qubit-local-files).
