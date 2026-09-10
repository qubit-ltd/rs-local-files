# Qubit Local Files Complete Filesystem Design

[中文设计文档](local_file_system_design.zh_CN.md) ·
[User guide](user_guide.md) · [README](../README.md)

> Status: normative design specification for qubit-local-files 0.5.0
>
> Last updated: 2026-09-11

The [Simplified Chinese design](local_file_system_design.zh_CN.md) is the
normative source for this English counterpart. Public APIs, platform
implementations, tests, READMEs, and user guides must remain semantically
aligned with it.
They describe the intended completed system. “Must”, “must not”, and “should” express normative
requirements. Non-semantic modifiers such as a particular `const` or `inline`
annotation are coding-policy details rather than API contracts.

## 0. Terminology

| Term | Meaning in this design |
| --- | --- |
| Host | The local namespace visible to the process through the operating system. |
| Rooted | A local namespace anchored by an opened directory descriptor or handle and presented with its own virtual `/`. |
| namespace path | A path accepted or returned by one `LocalFileSystem`, interpreted only in that instance's coordinate system. |
| Host diagnostic path | A best-effort Host-side description of a Rooted anchor; it grants no authority. |
| PWD | The applicable current directory: process-global for Host and instance-owned for Rooted. |
| authority | The operating-system object that grants namespace access; for Rooted it is the opened root handle. |
| native path/name | `Path`/`OsStr` data that preserves platform-native bytes or code units. |
| canonical component | One native name reversibly encoded as UTF-8 for transport across abstraction layers. |
| publication | A state transition that makes content or a name visible to other observers of the namespace. |

Unless explicitly qualified as physical or diagnostic, “absolute path”,
“root”, and path outputs refer to the owning `LocalFileSystem` namespace.

## 1. Positioning

`qubit-local-files` is a synchronous, native, application-facing local
filesystem capability layer. It serves applications, provider adapters such as
`qubit-fs-local`, and libraries that need Rooted authority, reliable
publication, temporary resources, recursive traversal, or structured failure
states. It is neither a rename of `std::fs` nor a duplicate of `qubit-fs`.

The crate turns the error-prone parts of cross-platform local filesystems—path
authority, symbolic links, atomic publication, durability, partial success,
and resource ownership—into one explicit object model.

## 2. Design Principles

### 2.1 State is explicit

`LocalFileSystem` is stateful. Rooted owns a virtual PWD; Host observes the
process PWD only when an operation binds a relative path. Each instance owns
its default options and symbolic-link policy. All state that affects an
operation must be observable; no hidden option set or resource cap may compete
with caller configuration.

### 2.2 Callers own policy

Callers choose depth, entry, byte, open-handle, deadline, retry, and name-attempt
budgets through options. The initial budget is unbounded. The crate validates
explicit options, reports structured limit failures, prevents internal cycles,
and maps operating-system resource errors without inventing business limits.

### 2.3 One filesystem, one coordinate system

Every instance has one namespace. Absolute paths start at its namespace root;
relative paths start at one operation-time PWD snapshot. Rooted presents a
virtual filesystem rather than requiring a special root-relative path type.

### 2.4 Handles grant authority

Rooted containment is defined by the directory descriptor or handle opened by
the constructor. A canonicalized string or diagnostic path must never be used
as proof of containment.

### 2.5 Lexical rules and native resolution are separate

The crate binds Host operands without folding native `.`/`..` traversal and
defines Rooted PWD and virtual-root lexical semantics. Real directory access,
links, reparse points, permissions, and
identity checks use descriptor- or handle-relative operating-system primitives
where available.

### 2.6 Partial success is structured

Copy, rename, writer commit, and temporary persistence may fail after a
namespace change. Their dedicated outcomes and failures retain the strongest
proven state. Recursive create/delete report `PublicationIncomplete` and the
failed path when a more detailed state is not available.

### 2.7 Native paths are lossless

Public APIs use `Path` and `OsStr`. Only the explicit path codec produces
canonical UTF-8 transport text; lossy conversion is forbidden.

### 2.8 The foundation provides mechanisms, not application policy

Authentication, tenants, user authorization, global synchronization, and
provider registries remain outside this crate.

## 3. Goals and Non-goals

### 3.1 Goals

The design provides one Host/Rooted type, virtual absolute paths, explicit
instance configuration, handle-relative containment, uniform read/write/walk/
copy/rename/temp operations, structured publication facts, and a clean adapter
boundary for `qubit-fs-local`.

### 3.2 Non-goals

It does not define URIs or remote protocols, depend on `qubit-fs`, bypass OS
permissions, choose business budgets, provide async I/O, promise persistence
beyond the OS contract, eliminate all races in attacker-writable directories,
or prevent crossing mounts and devices by default.

## 4. Dependency Boundary

```text
local applications
       │
       ▼
qubit-local-files
       ▲ native Path / options / outcomes / errors
       │
qubit-fs-local
       ▲ FileSystemSpi
       │
   qubit-fs
```

Platform algorithms live here. Adapters must not duplicate path codecs,
symbolic-link traversal, publication, or Rooted containment.

## 5. Core Object Model

### 5.1 LocalFileSystem

Conceptually, `LocalFileSystem` consists of an immutable, clone-shared
`Arc<LocalAuthorityCore>` and instance-owned `current_directory`, symlink policy,
and default options for read, write, list, copy, create-directory, delete,
rename, temporary file, and temporary directory operations. The exact source
layout may differ, but these invariants must hold:

```text
pub struct LocalFileSystem {
    // Immutable after construction and shared by clones and open resources.
    authority: Arc<LocalAuthorityCore>,

    // Instance-owned snapshots copied by clone.
    current_directory: PathBuf,
    symlink_policy: LocalSymlinkPolicy,
    default_read_options: LocalReadOptions,
    default_write_options: LocalWriteOptions,
    default_list_options: LocalListOptions,
    default_copy_options: LocalCopyOptions,
    default_create_directory_options: LocalCreateDirectoryOptions,
    default_delete_options: LocalDeleteOptions,
    default_rename_options: LocalRenameOptions,
    default_temp_file_options: LocalTempFileOptions,
    default_temp_directory_options: LocalTempDirectoryOptions,
}
```

- authority and its capability snapshot are immutable;
- PWD and defaults belong to one instance and are copied on clone;
- defaults are never placed in the shared `Arc`;
- mutating a clone never mutates another clone's configuration;
- there is no extra instance-level walk or copy hard limit; and
- a Rooted filesystem has exactly one authority participating in operations.

### 5.2 LocalAuthorityCore

The authority core owns namespace kind, the opened root, diagnostic anchor,
capability snapshot, objective filesystem limits, and shared platform resource
state. Caller budgets, options, PWD, and business authorization remain outside
that core.

### 5.3 Construction

```text
impl LocalFileSystem {
    pub fn host() -> LocalResult<Self>;
    pub fn rooted(root: &Path) -> LocalResult<Self>;
}
```

`host()` neither reads nor caches the process PWD. It remains constructible, and
absolute-path operations remain usable, while the PWD is temporarily unreadable.
Only operations which bind a relative path read it, and failures retain the
actual operation and operand. `rooted(root)` resolves one Host-native constructor
path, opens one authority, then exposes virtual `/` with PWD `/`. Its constructor
path is diagnostic-only after opening. A target without the required rooted
containment primitives returns `Unsupported`; optional publication and durability
capabilities do not prevent construction.

The conceptual split is also an ownership rule. A clone may share the immutable
authority and capability snapshot, but it receives its own PWD, link policy, and
nine default option values. A Rooted instance must not retain a second root
handle for a different operation path, and a Host instance must remain usable
for absolute paths when the process PWD cannot temporarily be read. The
constructor path of a Rooted instance is retained only as a diagnostic hint;
reopening that path after a rename or replacement would violate the authority
contract.

## 6. `LocalFileSystem` Public API

### 6.1 State and capabilities

```text
impl LocalFileSystem {
    pub fn scope(&self) -> LocalFileSystemScope;
    pub fn current_directory(&self) -> LocalResult<PathBuf>;
    pub fn set_current_directory(&mut self, path: &Path) -> LocalResult<()>;
    pub fn symlink_policy(&self) -> LocalSymlinkPolicy;
    pub fn set_symlink_policy(&mut self, policy: LocalSymlinkPolicy) -> LocalResult<()>;
    pub fn diagnostic_root(&self) -> Option<&Path>;
    pub fn capabilities(&self) -> LocalFileSystemCapabilities;
    pub fn limits(&self) -> LocalFileSystemLimits;
    pub fn limits_at(&self, path: &Path) -> LocalResult<LocalFileSystemLimits>;
    pub fn space_at(&self, path: &Path) -> LocalResult<LocalFileSystemSpace>;
}
```

The public state-and-fact API consists of `scope`, `current_directory`,
`set_current_directory`, `symlink_policy`, `set_symlink_policy`,
`diagnostic_root`, `capabilities`, `limits`, `limits_at`, and `space_at`.
There is one getter/setter pair for each of the nine defaults. Setters validate
structural, scope, and known capability constraints early, but operation entry
points repeat relevant validation because explicit options and path-dependent
runtime facts can differ.

### 6.2 Default option getters and setters

```text
impl LocalFileSystem {
    pub fn default_read_options(&self) -> &LocalReadOptions;
    pub fn set_default_read_options(&mut self, options: LocalReadOptions) -> LocalResult<()>;
    pub fn default_write_options(&self) -> &LocalWriteOptions;
    pub fn set_default_write_options(&mut self, options: LocalWriteOptions) -> LocalResult<()>;
    pub fn default_list_options(&self) -> &LocalListOptions;
    pub fn set_default_list_options(&mut self, options: LocalListOptions) -> LocalResult<()>;
    pub fn default_copy_options(&self) -> &LocalCopyOptions;
    pub fn set_default_copy_options(&mut self, options: LocalCopyOptions) -> LocalResult<()>;
    pub fn default_create_directory_options(&self) -> &LocalCreateDirectoryOptions;
    pub fn set_default_create_directory_options(&mut self, options: LocalCreateDirectoryOptions) -> LocalResult<()>;
    pub fn default_delete_options(&self) -> &LocalDeleteOptions;
    pub fn set_default_delete_options(&mut self, options: LocalDeleteOptions) -> LocalResult<()>;
    pub fn default_rename_options(&self) -> &LocalRenameOptions;
    pub fn set_default_rename_options(&mut self, options: LocalRenameOptions) -> LocalResult<()>;
    pub fn default_temp_file_options(&self) -> &LocalTempFileOptions;
    pub fn set_default_temp_file_options(&mut self, options: LocalTempFileOptions) -> LocalResult<()>;
    pub fn default_temp_directory_options(&self) -> &LocalTempDirectoryOptions;
    pub fn set_default_temp_directory_options(&mut self, options: LocalTempDirectoryOptions) -> LocalResult<()>;
}
```

There is intentionally no public builder. Callers configure a constructed
instance through `&mut self` setters. Host `set_current_directory` delegates
directly to `std::env::set_current_dir`; it changes process-global state and does
not pre-read or pre-validate PWD. Rooted first resolves and validates a directory
under its authority, then atomically replaces its virtual PWD. A failed setter,
including an unsupported `FollowAcrossScope` policy, leaves the prior state
unchanged.

### 6.3 Paired operation entry points

```text
impl LocalFileSystem {
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata>;
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader>;
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader>;
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>>;
    pub fn read_prefix_with_options(&self, path: &Path, max_bytes: usize, options: &LocalReadOptions) -> LocalResult<Vec<u8>>;
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter>;
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter>;
    pub fn list(&self, path: &Path) -> LocalResult<LocalDirectoryWalker>;
    pub fn list_with_options(&self, path: &Path, options: &LocalListOptions) -> LocalResult<LocalDirectoryWalker>;
    pub fn copy(&self, source: &Path, destination: &Path) -> LocalCopyResult;
    pub fn copy_with_options(&self, source: &Path, destination: &Path, options: &LocalCopyOptions) -> LocalCopyResult;
    pub fn create_directory(&self, path: &Path) -> LocalResult<LocalCreateDirectoryOutcome>;
    pub fn create_directory_with_options(&self, path: &Path, options: &LocalCreateDirectoryOptions) -> LocalResult<LocalCreateDirectoryOutcome>;
    pub fn delete_file(&self, path: &Path) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_file_with_options(&self, path: &Path, options: &LocalDeleteOptions) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_directory(&self, path: &Path) -> LocalResult<LocalDeleteOutcome>;
    pub fn delete_directory_with_options(&self, path: &Path, options: &LocalDeleteOptions) -> LocalResult<LocalDeleteOutcome>;
    pub fn rename(&self, source: &Path, destination: &Path) -> LocalRenameResult;
    pub fn rename_with_options(&self, source: &Path, destination: &Path, options: &LocalRenameOptions) -> LocalRenameResult;
    pub fn create_temp_file(&self) -> LocalResult<LocalTempFile>;
    pub fn create_temp_file_with_options(&self, options: &LocalTempFileOptions) -> LocalResult<LocalTempFile>;
    pub fn create_temp_directory(&self) -> LocalResult<LocalTempDirectory>;
    pub fn create_temp_directory_with_options(&self, options: &LocalTempDirectoryOptions) -> LocalResult<LocalTempDirectory>;
}
```

The state-and-capability queries are intentionally independent: `scope()` and
`symlink_policy()` are infallible, while PWD, path-dependent limits, and space
remain fallible observations. `diagnostic_root()` is never an access path.
Each default has exactly one getter and setter; setters replace the complete
value after validation and do not merge with a previous value. Operation
methods repeat validation because an explicit options value and runtime path
facts may differ from the instance defaults.

## 7. Options and Defaults

### 7.1 Complete replacement

Every configurable operation has an ordinary entry point and a
`*_with_options` entry point. The former uses its instance default; the latter
uses its supplied complete value:

```text
effective_options = explicit_options.unwrap_or(instance_default_options)
```

No fields are implicitly merged, no instance hard cap is applied over explicit
options, and the smaller of two limits is never silently selected. To alter one
default for one call, callers clone that default and alter the clone explicitly.

```text
let options = filesystem
    .default_copy_options()
    .clone()
    .with_conflict(LocalCopyConflictPolicy::Overwrite);
filesystem.copy_with_options(source, target, &options)?;
```
`symlink_policy_override: None` means inherit the filesystem policy; it is not a
merge of two option objects. Metadata, capability, and space queries have no
spurious `*_with_options` form.

### 7.2 Initial semantic defaults

Initial defaults are: no read-open retry; `CreateNew`, no parent creation,
preferred atomicity, and non-required durability for writing; non-recursive,
inherited-link-policy, fail-fast lists; conflict/type-conflict failure,
no metadata preservation, `Auto` source mode, no parent creation, preferred
atomicity, and non-required durability for copying; non-recursive and
exists-error create/delete; no-overwrite, non-required-durability rename; and
PWD parent, default naming, and no parent creation for temporary resources.

| Options | Initial semantics |
| --- | --- |
| `LocalReadOptions` | No open retry |
| `LocalWriteOptions` | `CreateNew`, do not create parents, atomicity `Preferred`, durability `NotRequired` |
| `LocalListOptions` | Non-recursive, inherit symlink policy, `FailFast` |
| `LocalCopyOptions` | Fail on conflicts and type conflicts, preserve no metadata, `Auto` source, do not create parents, atomicity `Preferred`, durability `NotRequired` |
| `LocalCreateDirectoryOptions` | Non-recursive, error if present |
| `LocalDeleteOptions` | Non-recursive, error if absent |
| `LocalRenameOptions` | Do not overwrite, durability `NotRequired` |
| `LocalTempFileOptions` | PWD parent, default name format, do not create parents |
| `LocalTempDirectoryOptions` | PWD parent, default name format, do not create parents |

### 7.3 Resource-budget defaults

All resource limits use `Option` and initially equal `None`: depth, entries,
seen-name bytes, copied bytes, open directories, deadlines, and temporary-name
attempts. `None` always means that the caller set no budget. A reopen policy
matters only when an open-directory budget exists; the crate must not substitute
a hidden fixed handle threshold. `open_retry_timeout` explicitly authorizes
library retries: `None` and zero perform only the first attempt, while a positive
duration permits retries in that monotonic interval.

### 7.4 Option responsibilities

List/copy/delete expose `tighten_resource_limits(self, ceilings: &Self) -> Self`.
Each optional limit combines by minimum, treating `None` as unbounded and zero
as a real value. The receiver retains all non-resource behavior; this explicit
transformation does not change complete replacement by `*_with_options`.
List tightens depth/entries/open directories/seen-name bytes/deadline; copy
tightens depth/entries/bytes/open directories/deadline; delete tightens
depth/entries/pending-path bytes/deadline. The method performs no I/O or
validation and never restarts a deadline. In fs-local the request determines
behavior before provider ceilings are applied; filtered-list request counts
remain separate from the native provider walker count.

### 7.5 Value and time semantics

Options are owned values with private fields, getters, consuming `with_*`
methods, corresponding `without_*` methods for optional budgets, and at least
`Clone`, `Debug`, and `Default`. `new()` and `Default` have identical initial
semantics where `new()` takes no necessary parameter. Options neither retain a
filesystem nor read global configuration. A `deadline: Duration` starts at
operation entry, uses a monotonic clock, and starts a walker at `list()` rather
than its first `next()`. Copy deadlines are cooperative chunk boundaries, not
claims to cancel a system call already in the kernel. `LocalPersistOptions`
belongs to an existing temporary resource, not filesystem defaults, and
controls overwrite, parent creation, and durability. Durable temporary-file
persistence synchronizes file contents before publication and the destination
parent chain after publication. Temporary directories cannot prove that
arbitrary descendant contents were synchronized, so they reject `Required`
durability before namespace mutation and never report full durability for a
`Preferred` request.

## 8. Namespace, Virtual Root, and PWD

### 8.1 Unified call model

Namespace-absolute paths begin at their namespace root. Relative Host paths
bind to an operation-time process-PWD snapshot, preserving native dot/parent
components and directory intent. Rooted paths start at the instance virtual PWD
and lexically fold dot/parent components, rejecting escape beyond virtual `/`.
Empty input means PWD in both scopes. Rooted stores a normalized
namespace-absolute PWD; Host stores none. On Unix, `a/link -> ../b/inner`
makes Host `a/link/../config` access `b/config`, while Rooted's caller-path
folding accesses `a/config`. Host `missing/../config` fails when `missing`
does not exist. Windows behavior follows native resolution; drive-relative
input remains rejected.

### 8.2 Rooted mapping

```text
let filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
```

| Virtual path | Conceptual Host location |
| --- | --- |
| `/` | `/srv/app` |
| `/etc/hosts` | `/srv/app/etc/hosts` |
| `/var/data/a.txt` | `/srv/app/var/data/a.txt` |

For a Rooted authority opened from `/srv/app`, virtual `/`, `/etc/hosts`, and
`/var/data/a.txt` conceptually denote `/srv/app`, `/srv/app/etc/hosts`, and
`/srv/app/var/data/a.txt`. This is explanatory only: access is handle-relative,
never `diagnostic_root.join(...)`. `/srv/app/log` is still a *virtual* path and
therefore conceptually denotes `/srv/app/srv/app/log`. Windows drive, UNC, and
device prefixes are invalid Rooted syntax.

### 8.3 Host binding and Rooted normalization

Rooted resolution captures PWD only for a relative input, starts a component stack at
the namespace root or PWD, adds normal components, ignores `.` and empty
components, and rejects `..` at root before producing a namespace-absolute path.
It uses `std::path::Component` and native `OsStr`, never UTF-8 splitting.
Examples at PWD `/` are `"" -> "/"`, `"a/./b" -> "/a/b"`,
`"a/../b" -> "/b"`, and `".." -> InvalidPath`; at `/work/project`,
`"../../tmp" -> "/tmp"` whereas `"../../../tmp" -> InvalidPath`.

At PWD `/`:

```text
""                  -> "/"
"."                 -> "/"
"a/./b"             -> "/a/b"
"a/../b"            -> "/b"
".."                -> InvalidPath
"a/./.././../b"     -> InvalidPath
```

At PWD `/work/project`:

```text
"data.db"           -> "/work/project/data.db"
".."                -> "/work"
"../../tmp"         -> "/tmp"
"../../../tmp"      -> InvalidPath
"/tmp"              -> "/tmp"
```

### 8.4 Two-path operations

Resolution retains directory intent from trailing separators, trailing `.`, or
other native forms until operation type checking. A writer must not turn
`"missing/"` into a regular file. NUL, invalid prefixes, and values that cannot
be represented losslessly are rejected. Copy and rename capture exactly one PWD
snapshot if either operand is relative; if both Host operands are absolute they
do not read PWD, and any lexical failure yields `Unchanged` without native
mutation.

### 8.5 `set_current_directory`

Host changes the process-global PWD. Rooted changes only the instance's virtual
PWD after resolving a directory under its opened authority. A failed change
leaves the previous PWD intact.

### 8.6 Rooted root operations

Virtual `/` is addressable for metadata, list, limits, space, and temporary
parents. It is never a writer target, copy source/destination, delete or rename
operand, or persistence target; violations are `InvalidPath`.

### 8.7 Host paths

Host preserves native drive and prefix semantics, rejects drive-relative forms
such as `C:foo`, and does not support UNC paths in Windows Host conversion.
Device namespace support requires a complete implemented authority contract;
Host does not provide a containment boundary.

### 8.8 Non-UTF-8 and name boundaries

The resolver preserves native component bytes/code units and directory intent.
Trailing separators, a trailing `.`, and equivalent native directory forms are
checked after lexical normalization, so `missing/` cannot silently become a
regular-file target. For two-path operations, source and destination are bound
from one PWD snapshot; a lexical failure is returned before any parent creation
or other namespace mutation and is reported as `Unchanged`.

## 9. Symbolic Links and Reparse Points

### 9.1 Policy

```text
pub enum LocalSymlinkPolicy {
    Reject,
    FollowWithinScope,
    FollowAcrossScope,
}
```

`Reject` prohibits traversing a link; observing, deleting, or renaming the link
entry itself is not traversal. `FollowWithinScope` permits traversal only while
the resolved name remains in the filesystem namespace. `FollowAcrossScope` is
Host-only. Rooted defaults to `FollowWithinScope`, Host to `FollowAcrossScope`.

### 9.2 Link target paths

Link targets use the same component rules: relative targets begin at the link's
directory and Rooted absolute targets restart at virtual `/`, never Host `/`.

```text
/srv/app/link -> /etc
```

### 9.3 Cycles

Cycles return a structured path error and must not be hidden behind a caller-
invisible fixed expansion budget.

### 9.4 Final components

Final-link semantics are stable: metadata observes the link itself; a reader
follows an allowed target; `CreateNew` treats it as occupied; `Append` follows
it; `CreateOrReplace` follows and replaces its referent while retaining the
link; delete-file removes the link; delete-directory rejects it as the wrong
type; rename moves the link; copy source copies it; copy destination and temp
persist replace its entry. A platform that cannot provide these semantics must
return `Unsupported` or `RequirementNotMet`.

| Operation | Final symbolic link or reparse point |
| --- | --- |
| `metadata` | Observe the link entry itself |
| listed entry | Return link metadata; recursion follows policy |
| `open_reader` | Follow the content target when policy allows |
| writer `CreateNew` | Treat the link as an occupied entry |
| writer `Append` | Follow the target when policy allows |
| writer `CreateOrReplace` | Replace target content while retaining the link entry |
| `delete_file` | Remove the link entry itself, including directory links |
| `delete_directory` | Return a type error for a final link |
| `rename` | Move or replace the link entry |
| copy source | Copy the link entry itself |
| copy destination | Replace the destination entry itself |
| temporary persistence target | Install through rename and replace the target entry |

Windows Rooted link inspection, kind detection, and creation remain
handle-relative; copying a dangling or out-of-authority link never opens its target.
Walkers retain logical paths and detect directory-identity cycles.

## 10. Authority and Platform Security Model

### 10.1 One Rooted authority

Rooted owns exactly one opened root authority. Renaming or replacing the Host
path later must not redirect operations.

### 10.2 Diagnostic paths are not authority

Diagnostic paths may be stale and are never authorization evidence.

```text
let filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
assert_eq!(filesystem.diagnostic_root(), Some(Path::new("/srv/app")));
```

### 10.3 Platform primitives

Unix uses descriptor-relative operations and Windows uses opened-handle
relative primitives.

### 10.4 Lexical and operating-system responsibilities

Lexical checks provide early classification but do not replace native authorization.

### 10.5 Mounts and hard links

Mount and hard-link boundaries are not rejected by default. Temporary cleanup
uses captured identity to reject ordinary replacement but identity-check and
path deletion remain separate native operations; attacker-writable parent
directories are outside the guarantee.

Unix implementations prefer descriptor-relative `*at` operations, final-link
controls, Linux `openat2` containment resolution where available, and descriptor
metadata. Windows implementations use opened directory handles, handle-relative
resolution, reparse-aware metadata, file/volume identity, and native install
primitives. No missing primitive may be replaced by `canonicalize` plus a string
prefix comparison. Mount boundaries are allowed by default; hard-link aliases
between copy source and target are detected and rejected.

## 11. Common Operation Data Flow

Each operation captures policy, options, and needed PWD; validates capabilities;
binds Host or normalizes Rooted operands; completes provable preflight before destructive I/O; invokes
the Host or Rooted backend; then maps all results and failures back into the
namespace coordinate system. Open resources retain their own authority, path,
options, and PWD snapshots.

## 12. Path Output Contract

### 12.1 Primary paths are reusable virtual absolute paths

All public resource identity paths (`path`, `root`, `source_path`, `target_path`,
and `staging_path`) are namespace-absolute and reusable with their owning
filesystem.

### 12.2 Directory entries

```text
impl LocalDirectoryEntry {
    pub fn path(&self) -> &Path;
    pub fn relative_path(&self) -> &Path;
    pub fn diagnostic_path(&self) -> Option<&Path>;
    pub fn metadata(&self) -> &LocalFileMetadata;
}
```

A directory entry exposes its namespace-absolute `path`, path relative to the
walker root, optional diagnostic path, and metadata.

```text
path()            = "/assets/icons/add.png"
relative_path()   = "icons/add.png"
diagnostic_path() = best-effort Host-side hint, when available
```

### 12.3 Temporary and publication paths

Temporary and publication paths never expose an authority-relative private
representation.

## 13. Metadata, Capabilities, and Reading

### 13.1 Metadata

Metadata observes the final entry itself and returns kind, length, permissions,
reliable platform timestamps, and needed identity information. Absence is a
structured `NotFound`, not `Option`.

### 13.2 Readers

Readers own their native file, implement `Read + Seek`, and are not redirected
by later rename or PWD changes.
`read_prefix` reads at most the requested byte count and never silently loads a
whole file.

### 13.3 Filesystem limits and space

```text
pub enum SizeLimit {
    Maximum(usize),
    VariesByPath,
    Unknown,
}
```

Objective path limits are distinct from application budgets; Host
may report `VariesByPath`, while `limits_at` and `space_at` probe the nearest
existing authority location and retain unknown facts as unknown.

Numeric limits carry `LocalPathLengthUnit`: bytes on Unix and UTF-16 code units
on Windows. Unknown limits remain unknown; code-unit limits are never guessed
as UTF-8 byte limits.

## 14. Writer Publication

### 14.1 Modes

```text
pub enum LocalWriteMode {
    CreateNew,
    CreateOrReplace,
    Append,
}
```

`CreateNew` and `CreateOrReplace` stage beside the destination, write and flush
bytes, perform required file synchronization, install through a native rename,
and synchronize the parent when required. A publication completed before parent
sync failure is `Published`, not `NotPublished`. Append writes an existing regular
file directly; it cannot satisfy required atomicity and cannot roll back bytes.

### 14.2 Lifecycle and publication state

```text
pub enum LocalWriterState {
    Open,
    Committed,
    Aborted,
}

pub enum LocalWriteFailureState {
    NotPublished,
    Published,
    Indeterminate,
}
```

Writer lifecycle is `Open`, `Committed`, or `Aborted`; failure knowledge is
`NotPublished`, `Published`, or `Indeterminate` and is independent of lifecycle.
`Interrupted` and `WouldBlock` permit retry. Other stream errors prevent further
write/flush/commit but permit abort: staging retains `NotPublished`; append
retains `Published` after a successful nonempty write, otherwise `NotPublished`.
Vectored I/O delegates one native operation and may return a short byte count.

### 14.3 Staged publication

`LocalWriteMetadataPolicy` separates copying old metadata from destination
identity checks. `PreserveExisting` is the default: Unix retains its implemented
owner/mode/ACL/xattr preservation; Windows Host uses `ReplaceFileW` merging;
Windows Rooted retains portable permissions only. Preservation failure must
not silently fall back to staging metadata. `UseStaging` skips old-content
reads and metadata copying; Windows Host uses non-merging replacement.
Native staging creation may still inherit permissions. The caller explicitly
accepts possible access-control changes when selecting this policy.

### 14.4 Append

Append writes directly to an existing regular file, cannot provide required
atomicity, and cannot roll back bytes already written.

### 14.5 Commit errors

Both policies observe destination type/identity and revalidate before install.
Check and install remain separate, not atomic compare-and-swap. Commit applies
selected metadata before staging sync, then validates identity, installs, and
syncs parents. A metadata failure is pre-publication; a parent-sync failure
can be `Published`. CreateNew has no old metadata to copy and retains atomic
no-replace installation; Append performs direct writes under either policy.

## 15. Lazy Directory Walking

### 15.1 Walker

Walkers are lazy `Iterator<Item = LocalResult<LocalDirectoryEntry>>` values.

```text
Iterator<Item = LocalResult<LocalDirectoryEntry>>
```
They never pre-collect a directory tree, fix creation-time policy and authority,
and offer explicit depth, entry, seen-name-byte, open-directory, and deadline
budgets. `FailFast` stops at the first error; continuation returns each error and
continues only safe branches. Directory identity detects recursive followed-link
cycles while output retains logical link paths. Dropping a walker releases
resources only.

### 15.2 Budgets

`Reopen` closes and later reopens frames when an explicit open-directory budget
requires it; `Fail` reports ResourceLimit at that boundary. Zero open-directory
capacity is invalid. No such limit is implied when the caller leaves it unset.

Host listing keeps the requested namespace root in public entry paths even when
the access path follows a symbolic link; an optional diagnostic path may expose
the physical access location. Rooted and Host tree copies advance directory
readers lazily. A bounded copy may inspect one child ahead while retaining the
reader permit, does not promise entry ordering, and never turns the native
directory buffer into an application entry budget.

### 15.3 Error policy

The root entry is charged once at depth zero. Continuation mode yields the
current error and continues only branches whose authority and frame state are
still valid; it does not rescan indefinitely after concurrent additions or
deletions. A walker captures its policy and authority at `list()` creation,
and its deadline starts there rather than at the first `next()` call.

## 16. Copy

### 16.1 Unified entry point and outcome

Copy auto-detects or validates file/tree source mode. Preflight rejects aliases,
impossible guarantees, unsupported link policy, and invalid conflicts before
publication. File copy stages when atomicity is selected. Tree copy maintains
typed partial statistics and source/destination identities while respecting
depth, entry, byte, deadline, and handle budgets. It does not implicitly stop at
mounts or devices.

### 16.2 Failure contents

```text
pub type LocalCopyResult = Result<LocalCopyOutcome, LocalCopyFailure>;

pub enum LocalCopyFailureState {
    Unchanged,
    PartiallyPublished,
    Published,
    Indeterminate,
}
```

`LocalCopyOutcome` reports statistics, method, atomicity, durability, and
metadata preservation. `LocalCopyFailure` retains the underlying structured
error plus `Unchanged`, `PartiallyPublished`, `Published`, or `Indeterminate`
state and partial statistics.

### 16.3 Preflight

Copy statistics have the same meaning in Host and Rooted scopes: `files`
counts copied regular files and links, `directories` counts newly created
directories, and `bytes` counts regular-file bytes. `overwritten` includes
replaced entries and existing directories merged under `Overwrite`, including
the copy root. `Skip` still merges same-kind directories but does not count
those merges as overwrites.

### 16.4 File publication

When a writer creates missing parents with required durability, it synchronizes
each newly created ancestor after publication. Failure in that chain is reported
as `Published` with an incomplete publication error; the target bytes remain
observable and callers must inspect the typed state.

### 16.5 Tree copy

Tree copy uses bounded lazy traversal, preserves partial statistics, and does
not imply a mount or device boundary.

### Copy source modes

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
requires platform support and synchronization of the destination parent and
newly created ancestors. Windows link kind comes from no-follow source
metadata; removing a destination directory link must use the native directory
link removal operation and preserve its referent.

Failure retains requested and failing operands, partial statistics, and staging
context only when cleanup failed. Preflight includes destination-inside-source.
Copy never mutates its source or silently downgrades required semantics.

Statistics are portable across scopes: `files` counts copied regular files and
links, `directories` counts newly created directories, and `bytes` counts
regular-file payload bytes. `overwritten` includes replaced entries and
existing directories merged under `Overwrite`, including the copy root; a
`Skip` merge of a same-kind directory is not an overwrite. Failure retains both
requested operands, the failing namespace path, partial statistics, and staging
context only when cleanup itself failed.

## 17. Create, Delete, and Rename

### 17.1 Create directory

Recursive create and delete are likewise non-transactional. If they have already
changed an entry when they fail, they report `PublicationIncomplete` and the
first unfinished namespace-absolute path; otherwise they retain the original
kind. Create's `exists_ok` accepts only an existing directory.

### 17.2 Delete

File deletion
removes a non-directory or link; directory deletion is separate and recursion is
explicit. Recursive deletion accepts optional depth, discovered-entry, pending-path
byte, and cooperative elapsed-time budgets. The root counts as one entry at depth
zero. Pending bytes bound encoded paths in the work queue, excluding allocator
overhead and in-flight enumeration objects; children are charged before queuing.
Budget failures retain typed resource facts even after partial deletion. Native
defaults remain replaceable; provider adapters enforce mandatory ceilings.

### 17.3 Rename

```text
pub type LocalRenameResult = Result<LocalRenameOutcome, LocalRenameFailure>;

pub enum LocalRenameFailureState {
    Unchanged,
    Renamed,
    Indeterminate,
}
```

Rename always uses a same-authority native rename rather than a
copy-delete emulation, reports `Unchanged`, `Renamed`, or `Indeterminate`, and
cannot silently downgrade its atomic namespace transition. Rename binds both
operands to one PWD snapshot, supports explicit overwrite and durability, and
reports Renamed if the namespace change succeeded before durability failed.

The operation/type contract is stable across Host and Rooted scopes. `delete_file` returns
`IsDirectory` for an entity directory and removes a final symbolic link itself, including a link
to a directory. `delete_directory` returns `NotDirectory` for a regular file or final symbolic
link, in both recursive and non-recursive modes, and never removes that entry. `missing_ok` only
turns a missing requested root into `deleted = false`; a missing child or any other traversal
failure remains an error. Rooted `/` remains invalid. These checks are no-follow observations and
do not claim atomicity against an untrusted concurrent renamer.

Recursive create and delete are deliberately non-transactional. The root is
charged as one entry at depth zero, children are charged before queueing, and
pending-path bytes cover native encoded paths held by the scheduler (not
allocator overhead or in-flight directory buffers). `missing_ok` applies only
to a missing requested root; a missing child remains an error. A durability
failure after a successful namespace rename is reported as `Renamed` with an
incomplete-publication error, never as `Unchanged`.

## 18. Temporary Resources

### 18.1 Ownership and publication are separate

`LocalTempFile` and `LocalTempDirectory` retain cleanup responsibility through
`outcome::LocalTempSourceState`. Their `source_state()` getters return current
eligibility:

| Source state | Meaning and permitted operations |
| --- | --- |
| `Owned` | The guard can operate on the original temporary entity, subject to revalidating identity before each destructive action. Partial cleanup may already have removed contents. |
| `CleanupRequired` | The original entity has left the source; only private sandbox cleanup remains. Persist and keep are forbidden. |
| `Released` | No cleanup responsibility remains. Cleanup is idempotently successful; persist and keep are rejected. |
| `Indeterminate` | Source namespace authority cannot be proven. Persist, keep, explicit cleanup, and Drop deletion are forbidden. |

`outcome::LocalPersistFailureState` reports a different fact: this call's target
publication is `NotPublished`, `Published`, or `Indeterminate`. It never implies
source eligibility on its own. A source state is passed explicitly into each
error; stage and `io::ErrorKind` must not be used to reconstruct ownership.

`close()` on a temporary file closes only its content handle; it does not alter
source eligibility. `keep()` follows the same publication protocol as persist
but generates a sibling target outside the private sandbox. Drop is a silent,
best-effort cleanup attempt only for responsibility still proven by the source
state and must never delete an `Indeterminate` source.

### 18.2 Creation and paths

Temporary parents accept namespace-absolute or relative paths; an omitted
parent uses the filesystem PWD captured for creation. A relative parent keeps
that PWD snapshot, while an absolute Host parent requires no extra PWD query.
Resources retain their creating authority, link policy, and namespace-absolute
path independently of later filesystem configuration or PWD changes. Name
prefixes/suffixes reject separators, NUL, and portable reserved-name violations
before creating entries. Collision retries have no hidden maximum; callers may
set `max_attempts`, and non-collision native errors return immediately.

### 18.3 Private sandbox

Creation places the entity inside a private per-resource sandbox beneath the
selected parent. Its generated component is visible in `path()`. Cleanup removes
the entity and then its empty sandbox. Persist and keep publish the entity out
of the sandbox; successful publication reports residual sandbox failures through
`LocalPersistOutcome::cleanup_state()` and `cleanup_error()`, without turning
publication into a retryable error. A sandbox reduces ordinary exposure but does
not establish an absolute synchronization boundary against concurrent writers.

### 18.4 Close, keep, and cleanup

`LocalTempFile::close(&mut self)` closes only the content handle. It does not
change source eligibility or relinquish cleanup responsibility. When still
Owned, a closed file can be persisted, kept, or cleaned. `keep(self)` uses the
same eligibility and publication protocol as persist but generates a sibling
target outside the sandbox and returns `LocalPersistOutcome`.

`cleanup(&mut self)` reports failure explicitly. Removing the source transitions
to `CleanupRequired`; removing the remaining sandbox transitions to `Released`.
Partial tree deletion retains `Owned` while the original source remains owned,
with its exact effect information and failed path. Drop is silent best effort
only for the responsibility still proven by the source state; it never deletes
an Indeterminate source. Dropping an error also drops its retained resource
under these same restrictions.

### 18.5 Persist and recovery

`persist(target)` and `persist_with(target, options)` consume the guard and
require namespace-absolute targets. Both resource types provide
`persist_at(self, base: &Path, target: &Path, options: LocalPersistOptions)` with
the same retained-resource error. The base must be an existing absolute
directory without literal `.`/`..`, resolved under the captured symlink policy.
The target must be nonempty and relative without a root or native prefix;
target parents follow Host native or Rooted lexical rules. The base is not a
new sandbox. Rooted `/` cannot be the publication target. Creation PWD remains
diagnostic context and never supplies a later target base.

Validate lifecycle eligibility before new target parsing. For an eligible
resource, invalid target parameters return `ResolveTarget` before source
sync/close or parent creation. An already disallowed source instead retains its
previous state; invalid arguments cannot restore Owned. Native rename/install
uses the creating authority, without cross-filesystem copy or target rollback.
Outcome reports namespace-absolute target, method, actual atomicity, actual
durability, and sandbox cleanup details. Required file durability synchronizes
content before publication and the parent chain afterwards. Directories reject
Required durability before publication because descendant contents cannot be
proven synchronized; Preferred never reports complete directory durability.

| Situation | Publication for this call | Source snapshot | Recovery |
| --- | --- | --- | --- |
| Owned target validation or parent preparation fails | `NotPublished` | `Owned` | Correct target, retry/keep, or cleanup. |
| No-replace conflict, source identity valid | `NotPublished` | `Owned` | Change target or explicit policy, retry, or cleanup. |
| Source identity mismatch before publication | `NotPublished` | `Indeterminate` | Read-only diagnosis; do not delete a replacement. |
| Native install effect cannot be proven | `Indeterminate` | `Indeterminate` | No automatic probe-and-restore or deletion. |
| Install succeeds, later synchronization fails | `Published` | `CleanupRequired` | Keep target, clean only sandbox; never republish or roll back. |
| Retry on CleanupRequired / Released / Indeterminate | `NotPublished` | Previous source state | Reject publication without erasing restrictions or earlier publication history. |

`LocalPersistError::state()` and `source_state()` are construction-time snapshots.
After `resource_mut()` changes the guard, obtain current eligibility from
`resource().source_state()`. `into_parts()` returns
`error::LocalPersistErrorParts<T>` with public fields:

```text
error: LocalFileError
resource: T
requested_target: PathBuf
resolved_target: Option<PathBuf>
stage: LocalPersistStage
state: LocalPersistFailureState
source_state: LocalTempSourceState
```

`requested_target` preserves the requested spelling and `resolved_target` is
present only when target binding established a namespace path. The state fields
remain failure snapshots after resource mutation. A subsequent
rejected call reports NotPublished for itself, not the disappearance of a target
published earlier. See [runnable recovery](user_guide.md#temporary-publication-and-recovery).

### 18.6 Temporary directory descendants

`child(component)` accepts exactly one valid normal name. `descendant(path)`
accepts only a nonempty relative path consisting of normal names; it rejects
absolute paths, native prefixes/drives, literal `.` and `..` components, and NUL.
`a/b` succeeds; `a/../b`, literal `a/./b`, and empty input fail, even if lexical
folding would stay beneath the temporary directory. The existing explicit-dot
check is necessary because `Path::components()` can hide interior literal `.`.
This follows the strict component contract of `LocalRelativePath`, not a second
normalizer. Ordinary Rooted filesystem inputs still allow contained lexical
parents; ordinary Host paths preserve native traversal order.

These helpers return namespace-absolute paths without opening or creating an
entry. A returned `PathBuf` grants no Rooted authority and proves nothing about
on-disk symlink targets. Regression cases are maintained in the [temporary-directory
public contract tests](../tests/local_temp_directory_tests.rs) and the runnable [cleanup example](user_guide.md#temporary-directory-cleanup-limits).

### 18.7 Identity and authority limits

The backend owns the one captured native identity and validates it before
destructive operations. An identity mismatch permanently makes the source
Indeterminate; writable-handle access must not bypass that state. Rooted
resources use their retained opened root handle, never reopen a diagnostic path.
Host uses the creation-bound native namespace path and identity. Root identity
loss/replacement must not be treated as a blanket missing-is-success case.

Identity validation and a later native operation can still race, and filesystem
identities may be reused. This contract does not eliminate all Host namespace
TOCTOU races or expand link-following permissions. Use trusted parents or
caller-owned synchronization for stronger cleanup guarantees.

### 18.8 Directory cleanup limits and accounting

`options::LocalTempCleanupLimits` owns optional `max_depth`, `max_entries`,
`max_pending_path_bytes`, and `deadline: Duration`, all unbounded in `new()` and
`Default`. Every field has a getter, `with_*`, and `without_*`. It intentionally
has no recursive or missing-ok switch. `LocalTempDirectoryOptions` stores it
through `with_cleanup_limits` and exposes `cleanup_limits()`.
`LocalTempDirectory::cleanup_limits()` reads it and `set_cleanup_limits()`
replaces it without I/O; `cleanup(&mut self)` keeps its existing signature.

Explicit cleanup and Drop use the same stored limits. Every call, including
Drop's at-most-one final attempt after explicit failure, starts a fresh budget.
Drop never falls back to unbounded limits. Deadlines are cooperative per-call
durations, not lifetime quotas or hard real-time interrupts for native I/O.
Validation of zero values and invalid combinations follows LocalDeleteOptions
and precedes any deletion. Callers must explicitly raise limits to allow a
larger retry budget.

The root counts as one entry at depth zero. Descendants are charged before
queuing; pending-path bytes count native-encoded paths retained in the work
queue, excluding allocator overhead, reader buffers, and current enumeration
objects. Sandbox release adds at most one native deletion outside source-tree
entry/path accounting. The deadline starts at cleanup entry, remains the same
through tree removal, and is checked before sandbox removal without restarting.

Temporary cleanup shares the unsorted post-order recursive deletion scheduler.
It removes child links without following their targets. It makes no ordering or
constant-memory promise. Partial deletion retains precise effects and Owned
eligibility for the remaining source; republishing would publish only remaining
contents. After source deletion, sandbox failure leaves CleanupRequired for a
sandbox-only retry. Concurrent additions can yield DirectoryNotEmpty; the
scheduler returns failure rather than rescanning forever. Enumeration, native,
and budget failures preserve their original cause and exact failing path.

The cleanup deadline is a fresh monotonic interval for each explicit cleanup
or Drop attempt. It is checked before sandbox release and is never restarted
for that release. Sandbox release is accounted separately from source-tree
entries and never changes the source publication state.

## 19. Structured Errors

### 19.1 Basic error domain

```text
pub type LocalResult<T> = Result<T, LocalFileError>;

pub struct LocalFileError {
    kind: LocalFileErrorKind,
    operation: LocalFileOperation,
    path: Option<PathBuf>,
    target: Option<PathBuf>,
    current_directory: Option<PathBuf>,
    reason: Option<&'static str>,
    source: Option<LocalFileErrorSource>,
    cleanup_error: Option<Box<LocalFileError>>,
}
```

`LocalFileError` stably exposes its kind, public operation, primary/target paths,
PWD context, reason, typed source, and cleanup error. Lexical failures preserve
the caller spelling and PWD snapshot; successful-resolution I/O errors use
namespace-absolute paths. Kinds include invalid path/options/state, missing and
existing entries, directory/type conflicts, permission, unsupported and unmet
requirements, explicit or OS resource limits, corruption, incomplete
publication, indeterminate state, and ordinary I/O. Display text is diagnostic
only. Dedicated copy, rename, commit, and persistence failures are recovery
objects; callers branch on their typed state.

### 19.2 Path context

Lexical failures retain the requested spelling and relevant PWD snapshot.
Failures after successful binding report namespace-absolute paths reusable with
the owning filesystem; diagnostic Host paths remain optional context only.

```text
kind: InvalidPath
operation: OpenReader
path: "../../../../etc/passwd"
current_directory: "/work/project"
reason: "path escapes the virtual root"
```

### 19.3 Error kinds

Kinds distinguish input/state errors, entry and type conflicts, permission,
unsupported or unmet requirements, resource limits, incomplete publication,
indeterminate state, corruption, and ordinary I/O.

### 19.4 Operations

The operation field names the stable public operation rather than an internal
helper or platform call.

### 19.5 Typed sources

Typed sources preserve codec, resource-limit, and I/O facts. Display text is
diagnostic and must not be parsed for recovery decisions.

### 19.6 Dedicated failures

`LocalFileEffectState` is a coarse effect vocabulary for the basic error type. The
`cause_kind()` query reports the best available underlying cause; the `effect_state()` query
reports `PartiallyApplied` for `PublicationIncomplete` and `Indeterminate` for `Indeterminate`
when that effect is encoded by the outer kind. Ordinary errors return `None` from
`effect_state()`, which means that the basic error has insufficient effect evidence and does not
mean `Unchanged`. The `Unchanged`, `Applied`, and exact recovery states remain owned by dedicated
copy, rename, writer, and persistence failure types.

## 20. Capabilities, Requirements, and Runtime Facts

### 20.1 Capability snapshot

`LocalFileSystemCapabilities` independently reports complete build protocols:

- Rooted operations;
- atomic rename;
- atomic replacement;
- the ability to attempt atomic no-replace temporary persistence;
- durable rename;
- durable file copy;
- durable writer publication;
- durable temporary-file persistence.

The temporary-persistence query is
`can_attempt_atomic_temp_persist()`. It describes whether this build and target
implement the atomic attempt protocol; it does not promise that arbitrary
source and target paths can complete atomically. Same-filesystem placement,
namespace policy, mount behavior, and runtime races still decide each outcome.

### 20.2 Requirements

```text
pub enum LocalAtomicityRequirement {
    Required,
    Preferred,
    NotRequired,
}

pub enum LocalDurabilityRequirement {
    Required,
    Preferred,
    NotRequired,
}
```

Capabilities do not prove that a particular mount, network filesystem, cache,
controller, or device persisted data. `Required` atomicity or durability is a
precondition; `Preferred` permits a typed non-atomic/non-durable outcome;
`NotRequired` avoids extra synchronization.

### 20.3 Runtime probing

Runtime facts that vary by path are probed against the selected authority and
path. Callers must combine the
capability snapshot with the typed outcome of the actual operation.

## 21. Clone, Concurrency, and Threads

### 21.1 Clone

Cloning shares immutable authority, copies Rooted PWD, policy, and all defaults,
and leaves Host clones observing global process PWD.

### 21.2 Configuration and operations

Setters use `&mut self`;
operations use `&self`; the crate provides no internal synchronization for
concurrent configuration. Caller-owned locking is required for one mutable instance.

### 21.3 Open resources

Readers, writers, walkers, and temporary resources remain valid after
the originating filesystem is reconfigured or dropped because they retain their
creation-time state.

## 22. Path and Filename Utilities

### 22.1 LocalPaths

```text
impl LocalPaths {
    pub const fn host() -> Self;
    pub const fn rooted() -> Self;
    pub fn from_canonical_components<'a>(
        &self,
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf>;
    pub fn to_canonical_components(&self, path: &Path) -> LocalResult<Vec<String>>;
}
```

`LocalPaths` converts namespace-absolute native paths to and from canonical
components without PWD or I/O; empty Rooted components mean virtual `/`.

### 22.2 LocalFileNames

`LocalFileNames` validates native components, applies explicit portable policy
and optional component limits, and creates safe random names without pretending
that every filesystem has a 255-unit limit.

### 22.3 LocalPathCodec

`LocalPathCodec` reversibly maps one
native component to canonical UTF-8, rejects aliases and malformed/lowercase or
unnecessary escapes, and has no lossy fallback on Unix or Windows.

These path helpers grant no filesystem authority.

## 23. Contract with `qubit-fs-local`

```text
qubit_fs::spi::Request
  → canonical logical path / abstract options
  → LocalFileSystem virtual path / native options
  → local outcome / failure / resource
  → qubit_fs outcome / error / session
```

`qubit-fs-local` maps abstract requests and canonical paths to native namespace
paths and complete options, then maps native typed outcomes and owned resources
back. It does not strip a Rooted leading slash, invent another root-relative
coordinate system, retain duplicate defaults, or reimplement codecs, containment,
link traversal, temporary ownership, or publication algorithms. Provider identity,
URI, registry, user metadata, and remote capability policy stay above this crate.
Non-exhaustive native variants require conservative fallback. Compatibility CI
must compile and test the adapter against this crate to detect variant drift.

The adapter must map `CopyMode::File` to `LocalCopySourceMode::Entry`,
`Tree` to `Tree`, and `Auto` to `Auto`. A resolved `Auto` request must overwrite
an entry/tree mode in native defaults while preserving independent budgets.

### Temporary failure adaptation

The native crate does not depend on the facade. The adapter takes both native
axes and first restores the
retained resource from named parts into its slot before building the portable
error. Persist and keep follow the same mapping:

| Native publication | Native source | Portable `PersistFailureState` | This target's effect |
| --- | --- | --- | --- |
| `NotPublished` | `Owned` | `NotPublished` | `Unchanged` |
| `NotPublished` | `Released` | `NotPublishedSourceReleased` | `Unchanged` |
| `NotPublished` | `Indeterminate` | `NotPublishedSourceIndeterminate` | `Unchanged` |
| `NotPublished` | `CleanupRequired` | `NotPublishedSourceCleanupRequired` | `Unchanged` |
| `Published` | `CleanupRequired` | `PublishedSourceRetained` | `Applied` |
| `Published` | `Released` | `PublishedSourceReleased` | `Applied` |
| `Published` | `Indeterminate` | `PublishedSourceIndeterminate` | `Applied` |
| `Indeterminate` | * | `Indeterminate` | `Indeterminate` |
| `Published` | `Owned` | `Indeterminate` | `Indeterminate` |

`Published + Owned` violates the native invariant and must be tested as
unreachable; the adapter conservatively handles it and future unknown variants
as Indeterminate. NotPublished + CleanupRequired is reachable on a repeated
native persist and must not be discarded merely because a facade normally
prevents the call. These effects describe this call's target publication, not
an aggregate transaction across parent creation or source cleanup.

`NotPublishedSourceIndeterminate` and `PublishedSourceIndeterminate` keep the
facade's resource lifecycle Indeterminate. Ordinary cleanup errors and invalid
target arguments cannot restore Owned or replace uncertainty with CleanupRequired.
`NotPublishedSourceCleanupRequired` retains CleanupRequired. A facade must keep
its previously confirmed `publication_target` through a rejected retry and
cleanup. Cleanup success yields PublishedSourceReleased when an earlier target
is known, otherwise NotPublishedSourceReleased for an unpublished resource.
Apply these rules to synchronous temporary files/directories and asynchronous
temporary files; cancellation keeps its existing conservative handling.
`qubit-mime` path staging retains explicit close/cleanup and independent primary
and cleanup failures; it does not need to adopt the persist workflow.

## 24. Direct Application Use

Applications choose Host for process-visible paths and Rooted when one opened
directory is the authority boundary. Configure instance defaults once, clone a
configured snapshot when needed, and use explicit options for one-call policy
changes. Recovery-sensitive code retains writer/copy/rename/persist failures and
branches on typed states.


### 24.1 Configure a Rooted application filesystem

```rust,no_run
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::policy::LocalSymlinkPolicy;

let mut filesystem = LocalFileSystem::rooted(Path::new("/srv/app"))?;
filesystem.set_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)?;
filesystem.set_default_list_options(
    LocalListOptions::new()
        .with_recursive()
        .with_max_entries(100_000),
)?;
filesystem.set_default_copy_options(
    LocalCopyOptions::new().with_max_bytes(1 << 30),
)?;
filesystem.set_current_directory(Path::new("/workspace"))?;

let walker = filesystem.list(Path::new("assets"))?;
for entry in walker {
    let entry = entry?;
    // entry.path() is a virtual absolute path that can be passed back directly.
    let metadata = filesystem.metadata(entry.path())?;
    assert_eq!(metadata.kind(), entry.metadata().kind());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

### 24.2 Replace options for one operation

```rust,no_run
# use std::path::Path;
# use qubit_local_files::LocalFileSystem;
# let filesystem = LocalFileSystem::host()?;
let options = filesystem
    .default_copy_options()
    .clone()
    .with_max_bytes(16 * 1024 * 1024);

filesystem.copy_with_options(
    Path::new("input.bin"),
    Path::new("/archive/input.bin"),
    &options,
)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

Explicit options are the complete configuration for this call; other instance
copy defaults are not implicitly merged back.

## 25. Internal Component Boundaries

```text
LocalFileSystem facade
├── instance configuration
│   ├── PWD
│   ├── symlink policy
│   └── operation defaults
├── virtual path resolver
├── authority core
│   ├── Host authority
│   └── Rooted single-handle authority
├── operations
│   ├── metadata/read
│   ├── writer publication
│   ├── lazy walk
│   ├── copy
│   ├── create/delete/rename
│   └── temp/persist
├── structured outcome/error
└── platform
    ├── Unix descriptor-relative primitives
    └── Windows handle-relative primitives
```

The stateful facade owns configuration and path binding. Host and Rooted
backends own authority-specific dispatch. `local::internal` contains native
publication, copy, path, and temporary mechanisms. `rooted` contains opened
authority primitives. `walk`, `writer`, and `temp` own their resource lifecycle
state. Internal extraction follows responsibility and testability; platform
algorithms must not be merged solely to reduce line count.

The only feature exposing deterministic fault injection is `test-support`; it
is disabled by default and is not application API. Private contract tests live
under `src/tests` and compile only for the crate's own test build. There is no
second public "internal test" feature and no public re-export of private
implementation contracts. Test hooks must not change the production state
model.

### Temporary resource core and deletion scheduling

A private `LocalTempResourceCore` owns shared path, backend, lifecycle, link
policy, and creation PWD responsibilities: identity validation, sandbox
release, public state projection, and error snapshots. Host and Rooted backends
each own their one applicable identity, without duplicate top-level identity
Options. The file wrapper owns the file handle, close, and synchronization;
the directory wrapper owns descendant helpers and cleanup limits. The core
holds no optional file handle and uses no file/directory boolean transaction.
Internal Owned, SandboxPending, Released, and Indeterminate map uniquely to
public source eligibility.

Directory cleanup validates identity then invokes the shared post-order delete
scheduler. The Rooted adapter borrows the retained root handle without reopening
a diagnostic path; Host uses its creation-bound path. Adapters provide existing
metadata/open/next/remove responsibilities without constructing another complete
LocalFileSystem. Type-specific publication steps stay in their wrappers. Other
reader/list sorting contracts remain independent; temporary cleanup must never
return to full collection and sorting.

### Shared copy scheduling contract

The shared scheduler alone owns and mutates the traversal stack. It constructs
child coordinates once, checks depth and deadline, and charges each descendant
entry once before backend processing. The operation entry charges the requested
root exactly once; remaining entry capacity is passed to the tree pipeline.
`CopyTreeBackend::process_entry` returns `None` for a completed/skipped entry or
`Some(Frame)` for one child directory. Backends cannot modify the stack.

Backends own native I/O, publication, byte accounting through `CopyBudget`, and
reader permits retained in frames. Statistics reflect completed effects even
when a later call fails. `finish_frame` performs post-order metadata work;
failure drops its consumed frame and every stacked ancestor. Dropping resources
releases readers and permits, but never rolls back published destination data.

## 26. Verification Strategy

### 26.1 API contract tests

External tests exercise public contracts; crate-internal tests cover genuine
crate-private contracts without widening production visibility.

### 26.2 Path property tests

Property tests cover lossless native path round trips and lexical invariants.

### 26.3 Rooted authority tests

Rooted tests retain one opened authority across diagnostic-path rename and
replacement, and cover link escape, cycles, and virtual-root restrictions.

### 26.4 Clone and lifecycle tests

Clone tests distinguish shared immutable authority from copied configuration;
resource tests verify that opened objects retain creation-time state.

### 26.5 Operation-state tests

Operation tests cover complete option replacement, budgets, partial effects,
typed recovery states, and deterministic fault injection.

### 26.6 Cross-platform and downstream validation

Configured platform jobs and downstream adapter checks validate target-specific
behavior and mapping completeness.

Verification follows contracts rather than line count:

- external tests exercise public success, error, policy, ownership, and
  publication states;
- crate-internal tests exercise real private contracts that public APIs cannot
  deterministically construct, without expanding public visibility;
- property tests cover native path round trips and lexical invariants;
- the exact bilingual README, user-guide, and design Rust examples are included as
  doctests, preventing copied-example drift;
- benchmarks represent codec, walk, handle-budget, copy, writer, Rooted writer,
  prefix-read, and Rooted deep-metadata workloads; deep-metadata fixtures use
  depths 1, 8, 32, 64, and 128 and compare Host with Rooted;
- Unix vectored-reader tests cover both default and `test-support` builds, while
  Windows tests cover the sequential fallback's progress and error behavior;
- bounded fuzz targets exercise codec, path, Host lifecycle, and Rooted
  lifecycle invariants; lifecycle targets use unique per-process sandboxes,
  bounded collision retries, and RAII cleanup, and never rely on ambient
  machine paths;
- CI is configured for Linux, Windows, and macOS behavioral tests; FreeBSD and Android are
  compile-checked;
- a scheduled and manually dispatchable compatibility workflow tests
  `qubit-fs-local` and `qubit-mime` against the checked-out revision and their
  complete local path-dependency closure;
- the project-level coverage configuration grants no whole-file exemption;
  state-machine branches are covered through public behavior, crate-private
  contracts, or narrow deterministic fault injection.

The contract matrix additionally covers Host/Rooted API symmetry, complete
options replacement, transactional setters, operation-time PWD, root operation
restrictions, diagnostic-root rename, link escape/cycles, every typed recovery
state, and presence and absence of explicit budgets. Required all-feature tests
and strict Clippy are validation gates; configured platform jobs alone are not
evidence that a particular change passed them.

Temporary-resource regression tests cover Host/Rooted × file/directory source
replacement: NotPublished + Indeterminate, rejected cleanup, replacement
surviving Drop, and invalid later targets unable to restore eligibility.
No-replace conflicts exercise Owned recovery. Deterministic test-support
injection covers Published + CleanupRequired after synchronization failure,
retained destination, and sandbox-only cleanup. Budget cases include empty,
wide, and deep trees, child links, partial deletion, sandbox failure, zero
boundaries, fresh budgets, Drop retaining limits, and a deadline that does not
restart. Strict descendant tests accept `a/b` and reject `a/../b`, literal
`a/./b`, and empty input. Lifecycle fuzz bounds steps and tree size and uses an
independent scratch parent that the harness finally reclaims, even when a guard
correctly relinquishes deletion. Benchmarks cover Host/Rooted, new/replaced
targets, metadata/durability policies, and 4 KiB, 1 MiB, and 16 MiB payloads;
cleanup covers a 10,000-child wide tree and a 64-level tree with 4 files per
level, using unbounded and fixture-accommodating explicit limits. Setup and
final disposal stay outside measurement; no fluctuating wall-clock gate is used.
Platform validation must be backed by actual CI records.

## 27. Security Guarantees and Explicit Limits

### 27.1 Guarantees

Rooted operations remain anchored to the opened authority and reject lexical or
followed-link escape. Native names remain lossless. Required guarantees are
validated before destructive work where proof is possible. Structured failures
preserve the strongest known recovery state.

### 27.2 Non-guarantees

The crate does not override OS permissions, prevent all TOCTOU races in a
directory writable by an attacker, forbid mounts/hard links, provide async
cancellation, or prove physical durability beyond operating-system primitives.
Applications must use trusted parent directories for cleanup-sensitive
temporary entries and add higher-level policy where those limits matter.

## 28. Design Completion Conditions

The design is complete only while public APIs and defaults match this document,
Host and Rooted tests cover their distinct authorities, structured failures are
preserved through downstream adapters, bilingual documentation stays aligned,
all published Rust examples compile, configured CI and coverage gates pass, and
configured release checks pass.
