# Qubit Local Files User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) ·
[Design](local_file_system_design.md) ·
[API reference](https://docs.rs/qubit-local-files)

## Purpose and Audience

This guide covers `qubit-local-files` 0.3 on Rust 1.94 or newer. It is for
applications that operate on the host filesystem or need operations restricted
to one opened directory. It is not a provider registry, a remote filesystem
API, or a replacement for provider-level logical paths. The crate is
synchronous; async applications should call it from an appropriate blocking
execution context.

## Operational contracts

`LocalFileSystem` validates static option combinations before resolving or
mutating paths. Copy failures report the strongest proven destination state;
`Unchanged` means this operation did not modify any destination entry, while `Indeterminate`
means the final destination state could not be established. A `read_prefix` error
keeps the outer `Read` operation even when opening the reader failed.

## Conceptual Model

```text
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

Permission observations describe native entry metadata, not the caller's effective access after
ACLs, mount policy, or other operating-system checks. On Unix, `unix_mode()` preserves the observed
permission and special bits; on Windows it returns `None`. `metadata()` and listing inspect the final
link entry, while a reader reports metadata from its opened content handle.

## Installation and Minimal Configuration

Add the crate to the application manifest:

```toml
[dependencies]
qubit-local-files = "0.3"
```

Choose the authority before configuring operation policy. Host mode uses the
process-visible namespace. Rooted mode opens one existing directory and treats
it as virtual `/`; the constructor path is only a Host-side diagnostic after
the authority is open.

```rust,no_run
use std::path::Path;

use qubit_local_files::LocalFileSystem;

let host = LocalFileSystem::host()?;
let rooted = LocalFileSystem::rooted(Path::new("workspace"))?;
assert!(host.diagnostic_root().is_none());
assert!(rooted.diagnostic_root().is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

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
| `open_reader` | Follows the content target when the effective policy permits it; `Reject` returns an error. |
| `CreateNew` writer | Treats an existing link as an existing entry. |
| `Append` writer | Follows the link and appends to its target. |
| `CreateOrReplace` writer | Follows the link, replaces its target, and preserves the link. |
| `delete_file` | Removes the link entry itself, including a link to a directory. |
| `delete_directory` | Rejects a final link with `NotDirectory`. |
| `rename` | Moves or replaces the link entry. |
| `copy` source | Copies the link entry itself. |
| `copy` target | Replaces the target link entry. |
| `temp persist` | Publishes by rename and replaces the target link entry. |

Readers and writers accept only regular-file content targets. A permitted link
must resolve to a regular file; directories and special files are rejected.

Listing follows directory links when the effective policy allows it. Returned
paths remain logical paths through the link (for example, `link/child`), and
recursive traversal detects directory-identity cycles. Depth counts logical
entries; crossing a link does not add another level.

## Scenario: write and inspect an export

An exporter must create `build/output`, publish `manifest.json` only after a
complete write, and inspect the result. The observable success condition is a
`Committed` writer outcome and the bytes read back from the published file.

```rust,no_run
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
publication conclusion (`LocalWriteFailureState`). `Interrupted` and `WouldBlock`
leave the writer usable for retry. Other stream errors prevent further writes
and commit: staging retains `NotPublished`; append reports `Published` if earlier
writes succeeded, otherwise `NotPublished`. Abort remains available for cleanup.
Commit failures can still report `Indeterminate`, so retain and inspect the
returned resource/error where recovery is required. Vectored writes may succeed
with a short count; advance through the buffers by the returned byte count.
When `create_parent` and required durability are selected, the atomic writer
creates missing ancestors and synchronizes each newly created directory after
publication; a failure at that point is reported as `Published` with an
incomplete publication error.

`LocalFileSystem::copy` selects file or directory behavior from source
metadata. Use `with_entry_source()` or `with_tree_source()` when the source
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

### Copy source modes

In both Host and Rooted, `files()` includes copied links, `directories()` counts
only newly created directories, and `bytes()` counts regular-file bytes.
`overwritten()` includes replaced entries and existing directories merged under
`Overwrite`, including the copy root. A `Skip` directory merge is not an overwrite.

| Mode | Regular file | Final link, including dangling links | Directory | Special file |
| --- | --- | --- | --- | --- |
| `Entry` | Copy content | Copy the link itself | `RequirementNotMet` | `Unsupported` |
| `Tree` | `RequirementNotMet` | `RequirementNotMet` | Copy the tree | `Unsupported` |
| `Auto` | Entry copy | Entry copy | Tree copy | `Unsupported` |

`with_entry_source()` selects one file or link entry; `with_tree_source()`
requires an actual directory. `with_source_mode(LocalCopySourceMode::Auto)`
explicitly resets either selection. Source-kind rejection happens before
creating target parents or changing the destination. Directory-qualified path
syntax is validated separately and can report `NotDirectory` before dispatch.
A final source link is never dereferenced by mode selection. Directory
links encountered inside a tree still follow the effective traversal policy;
mode selection does not change intermediate-link or tree-traversal semantics.

Directory-tree copies cannot provide required atomicity or durability; link
copies cannot provide required atomicity. Unsupported `Required` guarantees
fail with `RequirementNotMet` before destination mutation. Link durability
depends on platform support and synchronization of the destination parent and
newly created ancestors. Windows link copies preserve the source link's file/directory kind
even for dangling links and replace destination links without deleting their
referents.

## Walk and Temporary Resources

`LocalFileSystem::list` returns a lazy `LocalDirectoryWalker`. It opens and
advances directories on demand; its namespace-bound root, Options, symbolic-link
policy, PWD snapshot, and authority are fixed at creation. No depth, entry,
name-memory, deadline, or open-directory budget exists by default. When a
caller sets an open-directory budget, `Reopen` closes and later reopens active
frames as needed, while `Fail` returns `ResourceLimit` at the boundary. A
zero handle budget is invalid and returns `InvalidOptions`. Rooted enumeration
streams each directory instead of first collecting it into a vector. Dropping
the walker only releases handles. Host enumeration keeps the requested
namespace path as the public root even when a followed symbolic link points to
another physical directory; the optional diagnostic path may still identify
that physical access path.

Temporary files and directories own cleanup according to current source
eligibility. Each resource lives in
a private generated sandbox that is removed with the resource. Dropping them
performs silent best-effort cleanup; call `cleanup()` when the caller must
observe a cleanup failure. `keep` atomically publishes to a generated sibling outside
the sandbox and returns a `LocalPersistOutcome`; its cleanup state reports any
residual sandbox. With no explicit parent, creation uses the filesystem PWD
captured for that operation. `path()`, `keep`, and persistence outcomes all
return namespace-absolute paths for both Host and Rooted, so they can be passed
back to the same filesystem independently of later PWD changes. Persistence
failures retain the resource; publication and source eligibility together
determine which subsequent operations are permitted. Prefixes and suffixes are
checked before entry creation:
native separators, NUL, and portable reserved-name violations do not leave an
entry behind. Name-collision attempts are unbounded unless the caller sets
`max_attempts`.

## Temporary publication and recovery

A generated temporary file is published only after writing finishes. `persist`
and `persist_with` consume the guard and accept namespace-absolute targets;
`persist_at` supplies an explicit base for a relative target. `LocalPersistOptions::new()`
rejects an existing destination. Successful publication returns
`LocalPersistOutcome`: inspect its path, atomicity, durability, `cleanup_state()`,
and `cleanup_error()`. A residual sandbox is reported in the successful outcome;
it does not undo publication or authorize another attempt.

On failure, inspect two independent facts:

| Query | Values and meaning |
| --- | --- |
| `failure.state()` | `NotPublished`, `Published`, or `Indeterminate`: what this call established about target publication. |
| `failure.source_state()` | A snapshot of `Owned`, `CleanupRequired`, `Released`, or `Indeterminate` source eligibility when the failure was created. |
| `failure.resource().source_state()` | The retained resource's current eligibility, including changes made through `resource_mut()`. |

`Owned` permits another lifecycle attempt, subject to a fresh identity check.
It does not promise intact contents: a failed recursive cleanup may already
have deleted some descendants. `CleanupRequired` means the original entry left
the source and only its private sandbox remains the guard's responsibility.
Only sandbox cleanup is allowed. `Released` has no cleanup obligation and
`cleanup()` succeeds idempotently. `Indeterminate` forbids persist, keep,
cleanup, and Drop deletion because source authority cannot be proven.

| Failure situation | Publication | Source | Recovery |
| --- | --- | --- | --- |
| Invalid target or parent preparation failure from an owned guard | `NotPublished` | `Owned` | Correct the target and retry, keep, or clean up. |
| No-replace conflict with source identity intact | `NotPublished` | `Owned` | Select a different target or explicit overwrite policy; cleanup is also allowed. |
| Original source replaced before publication | `NotPublished` | `Indeterminate` | Diagnose only; never remove the replacement. |
| Native install result cannot be established | `Indeterminate` | `Indeterminate` | Reconcile externally without automatic deletion or restoring eligibility. |
| Installation succeeded, then file publication synchronization failed | `Published` | `CleanupRequired` | Keep the published target; clean only the sandbox. |
| Retry on `CleanupRequired`, `Released`, or `Indeterminate` | `NotPublished` | Previous source state | Reject the new publication; retain existing source restrictions and any earlier target fact. |

The source lifecycle is checked before parsing a new target. An invalid target
cannot turn an indeterminate resource back into an owned one. Errors describe
this call; a later rejected call does not erase an earlier published target.
`keep` obeys the same source rules. Temporary directories reject required
durability before publication because the guard cannot prove synchronization
of arbitrary descendants. File persistence can fail after publication during
parent synchronization; it never rolls the target back.

### Recover an occupied output name

This complete example stages a manifest beneath its own temporary parent,
provokes a deterministic no-replace conflict, and explicitly checks cleanup.
The existing output remains unchanged. Use the retained resource for a retry
only after confirming it is still `Owned`.

```rust
use std::io::Write;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalTempSourceState;

let filesystem = LocalFileSystem::host()?;
let parent_options = LocalTempDirectoryOptions::new()
    .with_parent(&std::env::temp_dir())
    .with_max_attempts(16);
let mut parent = filesystem.create_temp_directory_with_options(&parent_options)?;
let target = parent.path().join("manifest.json");
std::fs::write(&target, b"existing manifest")?;
let options = LocalTempFileOptions::new().with_parent(parent.path());
let mut temporary = filesystem.create_temp_file_with_options(&options)?;
temporary.write_all(br#"{"complete":true}"#)?;

let failure = temporary.persist(&target).expect_err("no-replace must reject an existing target");
assert_eq!(failure.state(), LocalPersistFailureState::NotPublished);
assert_eq!(failure.source_state(), LocalTempSourceState::Owned);
let mut parts = failure.into_parts();
assert_eq!(parts.state, LocalPersistFailureState::NotPublished);
assert_eq!(parts.source_state, LocalTempSourceState::Owned);
parts.resource.cleanup()?;
assert_eq!(parts.resource.source_state(), LocalTempSourceState::Released);
assert_eq!(parts.source_state, LocalTempSourceState::Owned);
assert_eq!(std::fs::read(&target)?, b"existing manifest");
parent.cleanup()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`into_parts()` returns `error::LocalPersistErrorParts<T>` with named fields
`error`, `resource`, `requested_target`, `resolved_target`, `stage`, `state`,
and `source_state`. The latter two fields are failure snapshots; cleanup in the
example changes the resource's live state but leaves the snapshot `Owned`.
Dropping the error or parts also drops its owned resource under the same source
and cleanup restrictions. Do not discard a retained error solely because its
publication is `NotPublished`.

## Temporary-directory cleanup limits

Configure `options::LocalTempCleanupLimits` at creation with
`LocalTempDirectoryOptions::with_cleanup_limits`. The value provides getters,
`with_*`, and `without_*` for `max_depth`, `max_entries`,
`max_pending_path_bytes`, and `deadline: Duration`. `new()` and `Default` leave
all four unbounded. It contains no recursive or missing-ok behavior switch.

```rust
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;

let filesystem = LocalFileSystem::host()?;
let limits = LocalTempCleanupLimits::new()
    .with_max_depth(8)
    .with_max_entries(1_024)
    .with_max_pending_path_bytes(1024 * 1024)
    .with_deadline(Duration::from_secs(30));
let options = LocalTempDirectoryOptions::new()
    .with_parent(&std::env::temp_dir())
    .with_cleanup_limits(limits);
let mut directory = filesystem.create_temp_directory_with_options(&options)?;
assert_eq!(directory.cleanup_limits(), limits);
assert!(directory.descendant(Path::new("a/b")).is_ok());
assert!(directory.descendant(Path::new("a/../b")).is_err());
assert!(directory.descendant(Path::new("a/./b")).is_err());
assert!(directory.descendant(Path::new("")).is_err());
directory.cleanup()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`cleanup_limits()` reads the retained value. `set_cleanup_limits(limits)` changes
it without I/O and controls all subsequent explicit cleanup and Drop attempts.
Each attempt receives a fresh budget with the same stored limits. After an
explicit failure, Drop may make at most one more attempt; it never switches to
unbounded cleanup. Increase limits explicitly when the remaining work needs a
larger budget. A deadline is a cooperative per-call duration, not a lifetime
allowance or an interrupt for blocked native I/O.

The source root has depth zero and counts as one entry. Discovered descendants
consume entry capacity before queuing. Pending-path bytes count only retained
native-encoded paths in the work queue, excluding allocator overhead, reader
buffers, and the currently enumerated object. Zero values and invalid
combinations follow `LocalDeleteOptions` validation before any deletion.
Sandbox release costs at most one additional native deletion, outside the
source-tree entry and path budgets. It uses the same call deadline: the clock
starts at cleanup entry and is checked again before sandbox removal, without
restarting after tree removal.

Cleanup uses unsorted post-order traversal shared with recursive deletion. It
removes a child link itself without visiting its target and does not promise a
deletion order or constant memory use. A failure after some children were
removed preserves partial-effect information and the exact failed path; source
eligibility remains `Owned` while the source tree remains owned. Retry cleans
the remaining tree, and republishing it would publish only remaining contents.
Once the source is removed, a sandbox failure leaves `CleanupRequired`; only
sandbox removal is retried. Concurrent additions may cause `DirectoryNotEmpty`;
the operation returns an error instead of rescanning forever.

`child` accepts one normal name. `descendant` accepts a nonempty relative path
of normal names and rejects roots, native prefixes/drives, literal `.` or `..`
components, and NUL. In particular `a/../b` and literal `a/./b` are rejected even
when lexical normalization could keep them inside the directory. These helpers
construct paths without I/O; they do not grant Rooted authority or validate
on-disk symlink targets. These stricter rules are separate from ordinary Rooted
path normalization and ordinary Host native traversal.

## Host paths, replacement metadata, and resource limits

### Bind Host paths without lexical folding

Host binds relative paths to one operation-time process PWD and retains native
dot components and directory intent. Given `a/link -> ../b/inner`, reading
`a/link/../config` on Unix reads `b/config` through Host, while Rooted folds
the caller's path and reads `a/config`. Host `missing/../config` fails if
`missing` does not exist; it cannot bypass that component. Rooted rejects
lexical traversal beyond virtual `/`. Host follows native root behavior,
including Unix `/..`; Windows drive-relative input such as `C:foo` remains
invalid. Host is not a containment boundary.

Default Host metadata performs one final metadata query without probing every
prefix. Explicit `Reject` still checks traversed links, including `link/..`.
Compare Host and Rooted with `std` on the same fixture using
`cargo bench --bench local_files -- deep_metadata`; timings depend on the
filesystem and path depth.

### Choose replacement metadata explicitly

`LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)` defaults to
`LocalWriteMetadataPolicy::PreserveExisting`. Use `UseStaging` when the
replacement should retain staging metadata. This can change access control;
staging still inherits whatever its native creation environment supplies.

| Platform and scope | PreserveExisting | UseStaging |
| --- | --- | --- |
| Unix Host/Rooted | Existing metadata preservation, including implemented owner/mode/ACL/xattr copying; failure is reported | No old-content read or metadata copy requested |
| Windows Host | Native `ReplaceFileW` metadata merge | Non-merging native replacement |
| Windows Rooted | Portable permissions only; no full ACL/owner preservation promise | Skip portable permission copying |

Both policies keep target type and identity checks. These checks are not an
atomic compare-and-swap with installation. CreateNew has no old metadata to
copy; Append writes directly under either policy. A preservation failure occurs
before publication, while parent-sync failure may occur after publication;
inspect the retained commit error and its state before retrying.

```rust
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
    .with_metadata_policy(LocalWriteMetadataPolicy::UseStaging);
assert_eq!(options.metadata_policy(), LocalWriteMetadataPolicy::UseStaging);
```

### Tighten budgets without changing behavior

List/copy/delete options expose `tighten_resource_limits(self, ceilings: &Self)`.
For each resource limit, `None` means unbounded and two finite values select
the minimum. Zero stays zero; operation validation still rejects invalid zero
handle limits. Deadlines remain durations measured from operation start.
Behavior, including recursion, overwrite and parent creation, stays with the
receiver. `*_with_options` still uses the supplied complete options value.

```rust
use qubit_local_files::options::LocalCopyOptions;

let requested = LocalCopyOptions::new().with_max_bytes(100);
let ceilings = LocalCopyOptions::new().with_max_bytes(10);
let effective = requested.tighten_resource_limits(&ceilings);
assert_eq!(effective.max_bytes(), Some(10));
```

In `qubit-fs-local`, provider list entry ceilings count native entries before
prefix filtering; request entry limits count returned entries after filtering.
A filter with no matches can still exhaust the provider ceiling. These are
per-operation limits, not aggregate quotas across concurrent requests.

### Publish a temporary resource against an explicit base

Both temporary guard types accept only namespace-absolute targets in
`persist` and `persist_with`. Use `persist_at` for a relative target:

```rust,no_run
use std::io::Write;
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;

let filesystem = LocalFileSystem::host()?;
let base = std::fs::canonicalize(std::env::temp_dir())?;
let mut temporary = filesystem.create_temp_file()?;
temporary.write_all(b"generated report")?;
let published = temporary.persist_at(
    &base, Path::new("report.txt"), LocalPersistOptions::new(),
)?;
# let _ = published;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The base must be an existing namespace-absolute directory without explicit
`.`/`..` components; the target must be nonempty and relative without a root
or native prefix. Target parents follow Host native or Rooted lexical rules.
The base does not establish another sandbox: the captured creating authority
and symlink policy still apply, even after a Rooted diagnostic directory is
renamed. Rooted `/` cannot be the final publication target. Once the source is confirmed
eligible, invalid parameters return `ResolveTarget` before source sync/close or parent creation and retain
the guard. Later failures may retain a closed file; inspect publication state.
Creating PWD is diagnostic context only, and changing process PWD never
changes the explicit target base. `keep()` still generates an absolute sibling
target. Use a collision policy appropriate to the application when publishing
to a fixed name.

## Scenario: inspect a closed temporary file

MIME detection and external tools often reopen a filename instead of borrowing
a writer. Stage bytes, close the native handle, invoke the path consumer, then
explicitly clean up. Closing retains cleanup ownership. This example uses a
file read as the consumer so the documented workflow runs without an external
program; an application can invoke its tool in the callback instead.

The error pair retains both the primary I/O failure and a cleanup failure.
Creation failures occupy the structured-error slot before any inspection runs.
Production applications may wrap these two channels in their own named error.

```rust
use std::io;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;

fn inspect_staged<R>(
    payload: &[u8],
    inspect: impl FnOnce(&Path) -> io::Result<R>,
) -> Result<R, (Option<io::Error>, Option<LocalFileError>)> {
    let filesystem = LocalFileSystem::host().map_err(|error| (None, Some(error)))?;
    let options = LocalTempFileOptions::new()
        .with_parent(&std::env::temp_dir())
        .with_max_attempts(16);
    let mut file = filesystem.create_temp_file_with_options(&options)
        .map_err(|error| (None, Some(error)))?;
    let staged = file.write_all(payload);
    file.close();
    let primary = staged.and_then(|()| inspect(file.path()));
    match (primary, file.cleanup()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err((Some(primary), None)),
        (Ok(_), Err(cleanup)) => Err((None, Some(cleanup))),
        (Err(primary), Err(cleanup)) => Err((Some(primary), Some(cleanup))),
    }
}

let bytes = inspect_staged(b"payload", |path| std::fs::read(path))
    .expect("staging, inspection, and cleanup should succeed");
assert_eq!(b"payload", bytes.as_slice());
```

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

## Recursive deletion budgets

`LocalDeleteOptions` accepts `with_max_depth`, `with_max_entries`,
`with_max_pending_path_bytes`, and `with_deadline`. All are unbounded by default;
matching `without_*` methods remove individual limits. Budgets apply to recursive
directory deletion. The requested directory counts as one entry at depth zero.
Every discovered child consumes entry capacity before entering the work queue;
its native encoded path length consumes pending-path capacity until popped.
This queue limit excludes allocator overhead and in-flight enumeration objects. Both Host
and Rooted enumerate lazily with at most one directory reader open at a time.
Deadlines are checked between native operations and cannot interrupt blocked I/O.

Budget exhaustion before deletion retains `ResourceLimit` and typed resource
facts. After any entry was removed, the error is `PublicationIncomplete` and
still retains those facts; deadline errors retain `TimedOut` as their I/O kind.
Inspect both the effect classification and the cause before retrying.

The compatibility queries make those two dimensions explicit:
`LocalFileError::cause_kind()` reports the best known underlying cause, while
`LocalFileError::effect_state()` returns `Some(LocalFileEffectState::PartiallyApplied)`
for `PublicationIncomplete` and `Some(LocalFileEffectState::Indeterminate)` for
`Indeterminate`. Ordinary errors have no inferred effect and return `None`; that
value does not mean `Unchanged`. The dedicated copy, rename, writer, and persist
failure types remain the authority for their precise recovery states.

Instance default Options are convenience configuration, not mandatory ceilings:
explicit `*_with_options` replaces them completely. For a provider policy that
requests cannot loosen, configure `qubit-fs-local::LocalResourcePolicy` instead.

Deletion has a strict operation/type contract: `delete_file` returns
`IsDirectory` for an entity directory, while `delete_directory` returns
`NotDirectory` for a regular file or final symbolic link. `missing_ok` applies
only when the requested root itself is missing. If recursive deletion removes
entries before failing, the `LocalFileError` retains `PublicationIncomplete`;
inspect `effect_state()` and `cause_kind()` separately before retrying. A basic
error whose `effect_state()` is `None` carries insufficient effect evidence and
must not be treated as `Unchanged`.


## Errors and Diagnostics

`LocalFileError` carries a `LocalFileErrorKind`, a `LocalFileOperation`,
namespace-absolute primary and target paths when available, the operation's PWD
snapshot, and an optional typed source. Physical paths are optional diagnostics
and never define Rooted authority. Publication operations use dedicated failure
types to preserve partial-success state.

`LocalPersistError` retains the temporary resource and its structured
`LocalFileError`; `state()` reports this call's publication and `source_state()` snapshots source
eligibility. Recovery requires both, and the live resource getter after mutation. Native
I/O errors are available through the structured error source when present.

The basic error exposes additive compatibility queries for callers that need
both dimensions without taking ownership of the error:

```rust,no_run
use qubit_local_files::error::{
    LocalFileEffectState, LocalFileError, LocalFileErrorKind, LocalFileOperation,
};

let error = LocalFileError::new(
    LocalFileErrorKind::NotFound,
    LocalFileOperation::Metadata,
);
assert_eq!(error.cause_kind(), Some(LocalFileErrorKind::NotFound));
assert_eq!(error.effect_state(), None);
assert!(!matches!(
    error.effect_state(),
    Some(LocalFileEffectState::Unchanged)
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`None` from `effect_state()` means that the basic error does not carry enough
evidence to infer a namespace effect; it is not an `Unchanged` result.

On Unix, `LocalFileReader::read_vectored` uses the file descriptor's native
vectored read path in both the default and `test-support` builds. Windows keeps
the sequential fallback required by its platform implementation; the fallback
preserves `Read` progress by returning accumulated bytes when a later buffer
read fails.

For Rooted metadata and similar operations, ordinary paths with no links use a
private one-pass directory cursor. Paths containing a link, a missing component,
or a native error return through the existing full resolver, preserving link
policy and authority checks. This is an implementation detail: callers should
rely on the same path and error contracts in either case.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| A Rooted operation rejects a path | Check whether lexical `..` or a followed link crosses virtual `/`, or whether the input contains a native prefix. Virtual absolute paths, `.`, and contained `..` are valid. Selecting `FollowAcrossScope` returns `InvalidOptions`. |
| A required guarantee is rejected | Inspect the selected filesystem capabilities and relax the requirement only if the application permits it. |
| Copy or rename returns an error | Inspect its typed failure state before retrying, cleanup, or treating the target as absent. |
| A temporary entry remains | Retain the resource and call its explicit lifecycle method; drop cleanup is best effort. |

## Limitations and Best Practices

CI is configured for Linux, Windows, and macOS behavioral tests; actual CI
results establish validation for a particular change. FreeBSD and Android are
compile-checked only. `capabilities()` reports the selected authority's build
capability snapshot; a Rooted instance caches it when opening the authority.
`scope()` lets integration code distinguish the two namespaces, and
`diagnostic_root()` exposes the non-authoritative Rooted anchor separately.
`limits()` reports `SizeLimit::VariesByPath` for the Host namespace; use
`limits_at(path)` to obtain a finite value for the filesystem containing that
path (or `Unknown` when probing is unavailable). Interpret both numeric limits
using `length_unit()`: Unix uses bytes and Windows uses UTF-16 code units, which
must not be treated as UTF-8 byte limits. Atomic rename, atomic replacement,
the ability to attempt atomic temporary persistence, durable rename, durable
file copy, durable writer publication, and durable temporary-file persistence
are reported independently because platform support differs.
`can_attempt_atomic_temp_persist()` describes an
implemented attempt protocol; same-filesystem placement and runtime namespace
conditions still determine the operation outcome. These flags do not prove
persistence on a particular mount or storage device.

The crate does not bypass operating-system permissions, block mount or hard-link
boundaries, eliminate every cross-platform race in an attacker-writable
directory, or make an unbudgeted operation consume bounded application
resources. Prefer Rooted mode for workspace authority, use trusted parents for
cleanup-sensitive temporary entries, set explicit budgets for untrusted trees,
inspect typed publication states after errors, and synchronize shared mutable
configuration in caller code.

## Tests and performance baselines

Registry dependencies are resolved with versions and checksums in `Cargo.lock`.
The downstream contract runner checks the coordinated local-files, fs, fs-local,
and mime dependency closure with `--locked`. Run performance measurements
separately from correctness tests:

```bash
# Compile the Criterion benchmark
cargo bench --locked --bench local_files --no-run

# Compare std, Host, and Rooted metadata on the same fixture
cargo bench --locked --bench local_files -- deep_metadata

# Measure new/replacement writers and wide/deep temporary-directory cleanup
cargo bench --locked --bench local_files -- '^(writer_scenarios|temp_directory_cleanup)/' --sample-size 20 --warm-up-time 1 --measurement-time 2
```

Benchmark comparisons require the same benchmark harness, machine, filesystem,
toolchain, and build profile. Record Host/Rooted, new/existing
target, metadata policy, durability, and payload for writer measurements, plus
wide/deep trees with unbounded/explicit limits for cleanup. Fixture creation
and final scratch-parent disposal belong outside measurement. Unsupported
platform-policy combinations must be identified as unsupported, not successful
samples. Criterion results describe trends and intervals; they are not CI
wall-clock gates or evidence of an improvement before measurement. Cleanup work
and queued-path memory depend on the directory tree.

Writer IDs use `writer_scenarios/{scope}/{target}/{metadata}/{durability}/{size}`;
cleanup IDs use `temp_directory_cleanup/{scope}/{shape}/{limits}`.

## Further Reading

Continue with the [README](../README.md), [中文用户手册](user_guide.zh_CN.md),
the [design document](local_file_system_design.md), or the
[API reference](https://docs.rs/qubit-local-files).
