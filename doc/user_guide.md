# Qubit Local Files User Guide

[中文](user_guide.zh_CN.md) · [README](../README.md) ·
[Design](local_file_system_design.md) ·
[API reference](https://docs.rs/qubit-local-files)

## Purpose and Audience

This guide covers `qubit-local-files` 0.5.0 on Rust 1.94 or newer. It is for
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

## Snapshots, fixed directories, and private temporary data

An editor can read UTF-8 text with `read_text`, or request a bounded
`LocalFileSnapshot` with `read_consistent_with_options`. The snapshot records
observations from the same opened file handle before and after reading. A
changed identity, size, or available native timestamps produce
`ChangedDuringRead`; this detects observed changes but is not an atomic snapshot
against an adversary that can write concurrently. Use the returned bytes as the
document version you edited, then use conditional publication when the update
must still match its original state.

`LocalFileSystem::host()` interprets paths in the process-visible Host
namespace. `LocalFileSystem::rooted(root)` and
`open_directory_with_options` retain an opened directory authority; paths below
that handle use namespace-relative names. Public `path()` values describe that
namespace and are not native authorization tokens. In particular, a Rooted
path is virtual: convert application wire formats at the application boundary,
and do not pass it to a native API. A sealed temporary file's `native_path()`
is available only for Host resources; it is a convenience path observation,
not a permanent binding if another actor can rename entries.

For a shared cache or generated artifact, request
`LocalPrivacyRequirement::Required` when creating a temporary file or
directory. The guarantee is checked during creation; creating a public object
and tightening permissions later does not satisfy it. A temporary file can be
sealed with `LocalSealOptions`; the returned `LocalSealedTempFile` retains a
read-only handle for observation and cleanup, but exposes neither a byte-stream
reader nor a mutable native handle. Reopen its namespace path through the same
filesystem authority when bytes must be read. File
sync and namespace durability are separate requirements: syncing the file does
not by itself promise that a later rename is durable.

For a directory backup, create a `LocalStagedDirectory`, populate it through
its fixed directory handle, and opt into validation rules only when needed.
Tree validation can reject links, special entries, or external hard links and
can enforce an explicit resource budget. These checks are independent: the
default behavior preserves legal links rather than following them. Publishing
consumes the stage and uses no-replace semantics. Inspect mutation and
publication separately on errors. A retained tree recovery location contains
only a namespace path and optional opaque identity, not an authority; reopen it
through the same parent and verify identity before acting on it. Explicit
`retain_in_place` transfers responsibility without renaming or publishing.
`apply_access_policy` is a separate `&mut` operation: `Preserve` makes no native
change, while the two current-user policies update only identity-checked files
and directories. If an item fails after earlier entries changed, inspect the
error's `access_state()`; `Mixed` reports partial conversion and the stage
remains with the caller. The library does not claim to roll back a whole tree.

`verify_entries()` compares the observed entries with the expected set exactly;
include each link entry, but do not add descendants reachable through an alias.
Duplicate expected paths return `InvalidOptions`. A finite depth limit checks
the complete tree and returns an error if any entry exceeds that depth; it does
not silently truncate validation. Expected-path bytes are charged before
indexing, and verification shares the configured entry, depth, path-byte,
open-directory, and deadline budgets. Special entries are accepted by default
and rejected only when that rule is enabled. Observations detect changes at
visited entries but are not an atomic snapshot. Publishing without validation
options performs no recursive scan.

Migration note: when enabling exact validation, expected sets should contain
only actual entries (including a link itself), never alias descendants. Keep
ordinary regular-file readers and temporary-resource calls unchanged; no new
tree-validation option is required for those paths.

This staged backup example captures the generated name and identity before
publishing. If the destination already exists, the returned stage is explicitly
retained and reopened beneath the same parent; cleanup only proceeds after the
identity check succeeds:

```rust,no_run
use std::ffi::OsStr;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::options::{
    LocalDirectoryOpenOptions, LocalTempDirectoryOptions, LocalTreePublishOptions,
    LocalTreeValidationOptions,
};

fn create_backup(parent: &LocalDirectoryHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
    let mut manifest = &b"backup version 1\n"[..];
    stage.write_file_new(Path::new("manifest.txt"), &mut manifest, 1024)?;
    let name = stage.relative_name().to_os_string();
    let identity = stage.identity().cloned().ok_or("stage identity unavailable")?;

    match stage.publish_new(parent, OsStr::new("backup-current"), &LocalTreePublishOptions::new()) {
        Ok(outcome) => {
            assert_eq!(outcome.publication(), qubit_local_files::outcome::LocalTreePublication::Published);
        }
        Err(error) => {
            let publication = error.publication();
            let (cause, stage, retained) = error.into_parts();
            if publication != qubit_local_files::outcome::LocalTreePublication::NotPublished {
                if let Some(stage) = stage {
                    drop(stage.retain_in_place());
                }
                for resource in retained {
                    drop(resource.retain_in_place());
                }
                return Err(cause.into());
            }
            if let Some(stage) = stage {
                let location = stage.retain_in_place();
                assert_eq!(location.identity(), Some(&identity));
                let reopened = parent.open_staged_directory(
                    Path::new(&name), &identity, &LocalTreeValidationOptions::new(),
                )?;
                reopened.cleanup()?;
            }
            for resource in retained {
                resource.cleanup()?;
            }
            return Err(cause.into());
        }
    }
    Ok(())
}

let filesystem = LocalFileSystem::host()?;
let parent = filesystem.open_directory_with_options(
    Path::new("backup-root"), &LocalDirectoryOpenOptions::new(),
)?;
create_backup(&parent)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The example's cleanup branch is a policy choice for a one-shot backup job. An
application that needs operator recovery should persist the namespace location
and identity, keep the owner context long enough to call `open_staged_directory`,
and avoid dropping a recovery owner before handoff is complete.

Tree recovery owners attempt identity-checked cleanup on Drop, while digest
conditional-write `LocalRecoveryFile` values are retained for caller-directed
recovery and are not automatically deleted. These are different ownership
contracts. Windows Rooted content-conditional replacement remains unsupported;
the crate does not claim otherwise.

## Byte stream protocols and standard I/O

`LocalFileReader` uses `ByteInput` and retains `std::io::Seek`.
`LocalFileWriter`, `LocalTempFile`, and the internal atomic writers use
`ByteOutput`. These marker traits are implemented automatically for
`Input<Item = u8>` and `Output<Item = u8>`, which provide `read_exactly` and
`write_fully`. Standard readers can be adapted with `into_std_read()`.

For a standard producer that must leave the writer available for publication,
use a borrowed adapter and end its scope before committing:

```rust,no_run
use qubit_io::{Output, OutputRef};
use qubit_local_files::LocalFileSystem;

let filesystem = LocalFileSystem::host()?;
let mut writer = filesystem.open_writer(std::path::Path::new("report.txt"))?;
{
    let mut standard = OutputRef::new(&mut writer).into_std_write();
    std::io::Write::write_all(&mut standard, b"report")?;
    std::io::Write::flush(&mut standard)?;
}
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

An owned `StdWrite<LocalFileWriter>` can instead be consumed with
`into_inner().commit()`. Adapter conversion does not flush or commit. `flush`
does not publish the destination and does not provide durable synchronization.
The native vectored paths remain available as inherent `read_vectored` and
`write_vectored` methods.

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
qubit-local-files = "0.5.0"
qubit-io = "0.19"
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

`open_reader_with_options` and `read_prefix_with_options` use their supplied
`LocalReadOptions` as a complete replacement for `defaults.read`. The combined
`open_observed_reader`, `read_all_with_options`, and `read_text_with_options`
operations accept optional nested read options: a present value completely
replaces `defaults.read`, while an absent value inherits it. Snapshot builders
create their own `LocalReadOptions::new()` and do not inherit `defaults.read`.
Other operation-specific `*_with_options` methods use their supplied options
as a complete replacement. These rules do not merge fields between option
values. To change one field while retaining others, clone the relevant default,
change the clone, and pass it explicitly.

For link handling, an explicit outer-operation policy takes precedence over
the selected nested read option's `symlink_policy`, which takes precedence over
the filesystem instance policy. Rooted rejects `FollowAcrossScope` before I/O.
Directory child handles combine denied component names with the parent's set;
a child cannot remove a parent's restriction, including when opened as `.`.

For example, if `defaults.read` sets `open_retry_timeout`, omitting nested read
options from `read_all_with_options` inherits that timeout. Passing
`LocalReadAllOptions::new().with_read_options(LocalReadOptions::new())` clears
it because the nested value replaces the whole read-options value; other
`LocalReadAllOptions` fields remain in effect.

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
When descent is not allowed, listing returns the link entry from its own
metadata without resolving its target; this includes dangling links in a
non-recursive Host listing. Recursive follow attempts can report dangling or
out-of-scope links. With `Continue`, a local child-open error is yielded and
other safe siblings remain available; global budget or deadline exhaustion
still terminates the walk.

## Scenario: write and inspect an export

An exporter must create `build/output`, publish `manifest.json` only after a
complete write, and inspect the result. The observable success condition is a
`Committed` writer outcome and the bytes read back from the published file.

```rust,no_run
use qubit_io::{Output, Streams};
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
writer.write_fully(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut bytes = Vec::new();
Streams::copy_input_to_output(&mut filesystem.open_reader(&path)?, &mut bytes)?;
let text = String::from_utf8(bytes)?;
assert_eq!(text, r#"{"complete":true}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Relative paths used by a multi-call operation are bound when that operation
starts. Copy and rename bind source and target from one current-directory
snapshot. `metadata` observes the final entry without following a final
symbolic link.

## Conditional write

Use a conditional write when an application reads document A, prepares an edit B,
and must avoid replacing a newer version written by someone else. Add the
optional feature and an algorithm implementation to the application manifest:

```toml
# BEGIN executable application dependencies
[dependencies]
qubit-local-files = { version = "0.5.0", features = ["digest"] }
qubit-io = "0.19"
sha2 = "0.11"
tempfile = "^3.27"
# END executable application dependencies
```

The following complete example creates A, computes its SHA-256 digest, and
conditionally replaces it with B. It also demonstrates the conflict path: after
C replaces A, commit fails and the application reads B from the retained
candidate copy.
The dependency feature is enabled on `qubit-local-files`; the consuming
application does not need to define its own `digest` feature.

```rust
# #[cfg(not(feature = "digest"))] fn main() {}
# #[cfg(feature = "digest")] fn main() -> Result<(), Box<dyn std::error::Error>> {
// BEGIN executable conditional write example
use qubit_io::{Output, Streams};
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{
    LocalWriteMode, LocalWriteOptions, LocalWritePrecondition,
};
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::{Digest, Sha256};

let directory = tempfile::tempdir()?;
// Reject also applies to links in the system temporary-directory path.
let path = directory.path().canonicalize()?.join("document.txt");
std::fs::write(&path, b"A")?;
let mut filesystem = LocalFileSystem::host()?;
filesystem.set_symlink_policy(LocalSymlinkPolicy::Reject)?;
let old_digest = Sha256::digest(std::fs::read(&path)?);
let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
let mut writer = filesystem.open_conditional_writer::<Sha256>(
    &path, LocalWritePrecondition::Content(old_digest), &options,
)?;
writer.write_fully(b"B")?;
let saved = writer.commit()?;
assert_eq!(saved.digest(), &Sha256::digest(b"B"));
assert!(saved.cleanup_errors().is_empty());
assert!(saved.retained_files().is_empty());

// Start a second edit from A; a competing save changes it to C before commit.
std::fs::write(&path, b"A")?;
let expected_a = Sha256::digest(b"A");
let mut conflicted = filesystem.open_conditional_writer::<Sha256>(
    &path, LocalWritePrecondition::Content(expected_a), &options,
)?;
conflicted.write_fully(b"B")?;
std::fs::write(&path, b"C")?;
let failure = conflicted.commit().expect_err("C no longer matches A");
assert_eq!(failure.reason(), qubit_local_files::error::LocalConditionalFailureReason::PreconditionMismatch);
assert_eq!(std::fs::read(&path)?, b"C");
let candidate = failure.candidate().expect("B recovery copy is retained");
let mut reader = candidate.open_reader()?;
let mut recovered = Vec::new();
Streams::copy_input_to_output(&mut reader, &mut recovered)?;
assert_eq!(recovered, b"B");
// END executable conditional write example
# Ok(())
# }
```

`LocalWritePrecondition::Absent` is for create-only publication and is available
only where the backend can install without overwriting a concurrently created
entry. `Content(digest)` requires `CreateOrReplace`; `Append` is invalid.
`LocalWriteOptions` also controls atomicity and durability. `Required` fails with
`Err` when the selected backend cannot meet the requirement. `Preferred` may
report weaker durability when synchronization is unavailable; it never downgrades
the requested atomic publication to an ordinary overwrite.

`NotRequired` skips the cleanup namespace sync. `Preferred` and `Required` still
attempt that sync after removing temporary content and the candidate copy; a
cleanup sync failure is reported through `cleanup_errors()` on the successful
outcome and does not undo the published file or change its durability result.
On Windows, `open_conditional_writer` rejects `Required` durability immediately
with `RequirementNotMet`; it does so before resolving or creating backend
resources, so no candidate namespace is created for an unsupported request.

On success, publication and required synchronization completed. Cleanup then
removes temporary content and the independent candidate copy; cleanup errors and
retained paths remain visible on the successful outcome. On error, inspect
`reason()`, `stage()`, and `publication()`; retain the error or transfer its
files with `into_recovery_files()`. Read `candidate().open_reader()` before
dropping the error when B must be recovered. Recovery handles are read-only and
dropping them does not remove their files. Explicit `abort()` and dropping an
uncommitted writer cancel publication and attempt cleanup; `commit()` errors
transfer recoverable state into the error.

This protocol is not a strict compare-and-swap. It checks old content before
publication and checks displaced content after publication, but another process
can modify an old inode through an already-open handle; digest ABA can also make
changed content look equal. Staging and recovery names may be visible in the
parent directory. Rollback is attempted at most once. Process abort or machine
crash can interrupt publication or cleanup; the library does not scan for
leftovers on restart. For backend availability, Windows Host supports
`PreserveExisting` metadata only for Content; Windows Rooted Content is
unsupported. Windows Rooted Content and Host Content with `UseStaging` return
`Unsupported` while opening the writer, before path resolution or staging/parent
side effects. Valid Host Content with `PreserveExisting` and `Absent` remain
supported. Rooted uses opened directory handles. Android and FreeBSD Content
are not claimed as supported.
On Windows, opening a conditional writer with `Required` durability returns
`RequirementNotMet` before path resolution or backend resource creation.
Recovery files and conditional commit errors are `Send + Sync`; a conditional
write outcome is also `Send + Sync` when its digest type supports those bounds.
This does not imply that an open writer is thread-safe.

Conditional writers capture the operation's PWD context when created. Later
commit, abort, and cleanup errors use that snapshot rather than querying a
possibly changed process PWD; physical Host paths remain optional diagnostics
and never replace Rooted namespace paths.

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
For Windows Rooted writers, an identity conflict found at the final check
before installation is `NotPublished`; staging remains available and the
competing destination is preserved. Do not infer the same state after a native
rename has already been attempted; inspect the typed commit failure.

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

Tree copy can remove an incompatible destination entry before it copies the
replacement. If a later step fails, inspect `LocalCopyFailure::state()` and
`partial_stats()`; `PartiallyPublished` records that removal even when no file
was counted as copied. An error alone does not imply that the destination is
unchanged.

With `with_create_parent()`, a copy creates missing parent directories before
opening the source and publishing the requested destination. If a later step
fails, those helper directories remain; they are not destination content
publication, and `LocalCopyFailureState::Unchanged` can still be correct when
the requested target was not changed. The library leaves them in place because
another operation may already be using them.

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

Copy options default to `LocalMetadataPreservePolicy::None`. When
`Permissions` is requested, file copies apply the source permissions to the
staged destination before its file synchronization and publication. A failure
before publication retains an existing destination; if a conflicting
destination was already removed, the typed failure reports that partial
effect. This copies permission metadata only, not source ACLs or all file
metadata.

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
that physical access path. A zero `max_depth` produces no entries and does not
read directory children; the same empty traversal applies when the depth,
entry, and seen-name-byte budgets are all zero. The deadline is checked while
the Host walker skips previously seen names and before its next directory
read. Rooted listing also checks it during path and symbolic-link resolution,
including each expansion step, and reuses the resolved starting directory.
These deadlines are cooperative: they cannot interrupt a blocked native
operation. Without a configured deadline, Rooted resolution has no hidden
symbolic-link expansion limit.
The walkers retain yielded names only when a seen-name-byte budget is configured
or `Reopen` may revisit frames under a finite open-directory budget. Without
that cache, concurrent directory changes follow native reader semantics and do
not provide a snapshot or global exactly-once guarantee; reopened frames still
suppress names already yielded.

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
entry behind. The complete prefix + generated name + suffix is checked before
parent creation. Portable names reject control characters, Windows-forbidden
punctuation, trailing spaces or dots, and reserved device names such as
`CON.txt`. Name-collision attempts are unbounded unless the caller sets
`max_attempts`.

## Temporary publication and recovery

A generated temporary file is published only after writing finishes. `persist`
and `persist_with` consume the guard and accept namespace-absolute targets;
`persist_at` supplies an explicit base for a relative target. `LocalPersistOptions::new()`
rejects an existing destination. Successful publication returns
`LocalPersistOutcome`: inspect its path, atomicity, durability, `cleanup_state()`,
and `cleanup_error()`. A residual sandbox is reported in the successful outcome;
it does not undo publication or authorize another attempt.
Before synchronizing a temporary file, the library verifies that the open
handle still identifies the owned temporary entry. Replacing the handle exposed
by `as_file_mut()` with another file cannot produce a durable success for that
other file. If the content handle was closed, the library reopens through the
creating authority and checks the reopened identity before synchronization.
The Windows native private-handle synchronous-I/O contract has not yet been
verified on a Windows runner; this behavior remains pending native validation.

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

If cleanup removes a temporary file but cannot remove its now-empty sandbox,
the cleanup error identifies the sandbox path; the deleted file is not listed
as a retained resource.

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
use qubit_io::Output;

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
qubit_io::Output::write_fully(&mut temporary, br#"{"complete":true}"#)?;

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
receiver. For these list/copy/delete operations, `*_with_options` still uses
the supplied complete options value.

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
use qubit_io::Output;
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;

let filesystem = LocalFileSystem::host()?;
let base = std::fs::canonicalize(std::env::temp_dir())?;
let mut temporary = filesystem.create_temp_file()?;
qubit_io::Output::write_fully(&mut temporary, b"generated report")?;
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
use qubit_io::Output;
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
    let staged = qubit_io::Output::write_fully(&mut file, payload);
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

Instance default Options are convenience configuration, not mandatory ceilings.
Direct per-operation `*_with_options` values replace the corresponding default;
combined reads follow the nested-option inheritance described in “Configure
Once, Override Deliberately.” For a provider policy that requests cannot loosen,
configure `qubit-fs-local::LocalResourcePolicy` instead.

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
compile-checked only. T3/T4 Windows cleanup and conditional-open behavior, and
LF-09 private-resource and LF-10 sealed-cleanup behavior, remain unverified
without native Windows runner results: Linux filters execute zero tests for
these cases, and Windows GNU `cargo check --lib` is compile evidence only.
`capabilities()` reports the selected authority's build
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
This crate's CI validates this crate's own package and does not run downstream
consumer tests. Adapter compatibility belongs to the adapter repository or a
separate integration job. Run performance measurements separately from
correctness tests:

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
