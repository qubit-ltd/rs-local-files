# Qubit Local Files Complete Filesystem Design

[中文设计文档](local_file_system_design.zh_CN.md) ·
[User guide](user_guide.md) · [README](../README.md)

> Status: normative design specification
>
> Last updated: 2026-09-03

This document and the [Simplified Chinese design](local_file_system_design.zh_CN.md)
are equal, normative specifications. Public APIs, platform implementations,
tests, READMEs, and user guides must remain semantically aligned with both.
They describe the intended completed system, not migration history or temporary
implementation details. “Must”, “must not”, and “should” express normative
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

The crate defines PWD, absolute/relative, `.`, `..`, and virtual-root lexical
semantics. Real directory access, links, reparse points, permissions, and
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

The design provides one Host/Rooted type, virtual absolute paths, explicit
instance configuration, handle-relative containment, uniform read/write/walk/
copy/rename/temp operations, structured publication facts, and a clean adapter
boundary for `qubit-fs-local`.

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

`LocalFileSystem` has an immutable shared authority core and instance-local
configuration. Cloning snapshots the virtual PWD, symbolic-link policy, and all
nine default option values. Rooted clones share only the immutable opened
authority. Host instances share process-global PWD only through the OS.

The authority core owns the namespace kind, opened Rooted handle, diagnostic
anchor, capability snapshot, objective filesystem limits, and platform state
needed by opened resources. Options, caller budgets, PWD, and business
authorization do not belong in the authority core.

`host()` creates a Host instance without reading PWD. `rooted(root)` resolves
its one Host-native constructor path, opens the authority, and then presents
virtual `/` with initial PWD `/`. Targets unable to provide the required
containment protocol return `Unsupported`; optional atomic or durable protocols
are reported through capabilities and outcomes instead of blocking creation.

## 6. `LocalFileSystem` Public API

The public surface has three groups:

1. State and facts: `scope`, `current_directory`, `set_current_directory`,
   symbolic-link policy, `diagnostic_root`, `capabilities`, `limits`,
   `limits_at`, and `space_at`.
2. One getter/setter pair for each of the nine default option values.
3. Ordinary operations using instance defaults and `*_with_options` operations
   using one complete caller-supplied option value.

Host `set_current_directory` delegates to `std::env::set_current_dir` and
therefore changes process-global state. Rooted validates and resolves a new
virtual directory before atomically replacing its instance PWD. Failed setters
must leave prior instance state unchanged.

## 7. Options and Defaults

Explicit options replace instance defaults completely; fields are never merged
implicitly and no hidden hard cap is layered on top. A caller wanting one
temporary change clones the default and changes that value explicitly.

Initial semantic defaults are conservative: reads do not retry; writes use
`CreateNew`, do not create parents, prefer atomicity, and do not require
durability; lists are non-recursive and fail fast; copies fail on conflicts,
preserve no metadata, auto-detect source kind, prefer atomicity, and do not
require durability; create/delete are non-recursive; rename does not overwrite;
temporary resources use PWD and default affixes.

Every optional resource budget starts as `None`, meaning the caller did not set
a limit. `open_retry_timeout` authorizes library retries: `None` and zero both
allow only the initial attempt, while a positive duration allows retries within
a monotonic window. Operation deadlines are elapsed durations converted to a
monotonic deadline at entry. Copy checks cooperative deadlines between bounded
chunks and cannot cancel a system call already entered.

Option values own their data, keep fields private, expose getters, consuming
`with_*` methods, matching `without_*` methods for optional budgets, and
`Clone`, `Debug`, and `Default` where appropriate. Option construction performs
no I/O; scope-, mount-, and cross-field validation occurs in setters or
operation entry points.

## 8. Namespace, Virtual Root, and PWD

Host relative paths bind one process-PWD snapshot per operation. Rooted
relative paths bind the instance PWD. Absolute paths start from the applicable
namespace root and do not query PWD. Empty input and `.` mean PWD; `..` removes
one component and returns `InvalidPath` if it would cross the root.

Rooted `/etc/hosts` means `/etc/hosts` inside the opened authority. It never
selects Host `/etc/hosts`. Windows drive, UNC, and device prefixes are invalid
Rooted syntax. Parsing uses native `Component` and `OsStr` values, never an
intermediate UTF-8 split.

Dual-path operations capture one PWD snapshot and bind both paths against it.
Rooted returns namespace-absolute paths that can be passed back to the same
instance even after PWD changes.

## 9. Symbolic Links and Reparse Points

Rooted defaults to `FollowWithinScope`; Host defaults to `FollowAcrossScope`.
Rooted supports only `Reject` and `FollowWithinScope`. Intermediate components
follow the selected policy. A Rooted absolute link target restarts from virtual
`/`; a target that escapes it is rejected.

Final components follow operation semantics: metadata inspects the link entry;
delete and rename act on it; copy copies the source link itself and replaces a
destination link entry; `CreateNew` treats it as occupied; append follows it;
`CreateOrReplace` follows and replaces the referent while preserving the link;
temporary persistence replaces the destination entry. Windows Rooted link
inspection and creation remain handle-relative and do not open dangling or
out-of-authority link targets merely to copy the link.

Walkers detect recursion cycles by underlying directory identity while
returning logical paths.

## 10. Authority and Platform Security Model

Rooted owns exactly one opened root authority. Renaming or replacing the Host
path later must not redirect operations. Diagnostic paths may be stale and are
never authorization evidence. Unix uses descriptor-relative operations and
Windows uses opened-handle relative primitives. Lexical checks provide early
classification but do not replace native authorization.

Mount and hard-link boundaries are not rejected by default. Temporary cleanup
uses captured identity to reject ordinary replacement but identity-check and
path deletion remain separate native operations; attacker-writable parent
directories are outside the guarantee.

## 11. Common Operation Data Flow

Every operation selects its complete options, validates static requirements,
captures PWD only when needed, normalizes namespace paths, resolves through the
selected authority and link policy, checks objective runtime facts, performs
native I/O, and maps results into typed paths, outcomes, or failures. Validation
that can prove a requirement impossible must happen before destructive
namespace mutation.

## 12. Path Output Contract

Primary paths in entries, resources, outcomes, and errors are reusable
namespace-absolute identities. Physical paths, when available, are optional
diagnostics. Directory entries keep the logical traversal path. Temporary
resources and persistence outcomes retain stable paths independent of later PWD
changes.

## 13. Metadata, Capabilities, and Reading

Metadata observes the final entry itself and reports type, length, permissions,
and platform-supported timestamps without lossy names. Readers are owned native
resources and preserve contextual path errors. Prefix reads stop at the caller
byte count without requiring EOF.

Host `limits()` may return `VariesByPath`; callers use `limits_at(path)` for the
selected filesystem. Numeric limits always carry `LocalPathLengthUnit`: bytes
on Unix and UTF-16 code units on Windows. Unknown facts remain `Unknown` rather
than being guessed or converted between units.

## 14. Writer Publication

`CreateNew` and `CreateOrReplace` stage in the destination directory. Append
modifies an existing regular file directly and cannot meet required atomicity.
Writer lifecycle state is separate from publication state. A failed flush,
install, parent sync, or cleanup retains the strongest proven destination and
staging facts. Required durability synchronizes file content and the necessary
parent namespace transitions before success.

## 15. Lazy Directory Walking

`LocalDirectoryWalker` fixes root, options, link policy, PWD snapshot, and
authority at construction, then opens and advances directories lazily. Caller
budgets cover depth, entries, name bytes, deadlines, and open directories.
`Reopen` may close and later reopen frames; `Fail` returns a resource limit.
Zero open-directory capacity is invalid. Drop releases resources but reports no
late traversal error.

## 16. Copy

Copy auto-detects or validates file/tree source mode. Preflight rejects aliases,
impossible guarantees, unsupported link policy, and invalid conflicts before
publication. File copy stages when atomicity is selected. Tree copy maintains
typed partial statistics and source/destination identities while respecting
depth, entry, byte, deadline, and handle budgets. It does not implicitly stop at
mounts or devices.

`LocalCopyOutcome` reports statistics, method, atomicity, durability, and
metadata preservation. `LocalCopyFailure` retains the underlying structured
error plus `Unchanged`, `PartiallyPublished`, `Published`, or `Indeterminate`
state and partial statistics.

## 17. Create, Delete, and Rename

Create supports single-level or recursive creation and explicit exists-ok
policy. Delete distinguishes file and directory operations, optionally accepts
missing paths, and uses explicit recursion. Rename captures one PWD snapshot,
supports explicit overwrite policy, reports durability, and retains
`Unchanged`, `Renamed`, or `Indeterminate` on failure. Recursive operations that
cannot provide a finer recovery object still identify incomplete publication
through a stable error kind and failed path.

## 18. Temporary Resources

A live temporary file or directory owns cleanup responsibility in an `Armed`
state. Every resource is created inside a private per-resource sandbox. Explicit
`cleanup` reports failure; Drop is silent best effort. `keep` atomically moves
the entry to a generated sibling and reports residual sandbox cleanup.
`persist` publishes to a caller target with explicit replacement/parent policy
and returns a typed outcome. Persistence failure retains the resource for retry,
inspection, keeping, or cleanup.

Prefixes and suffixes reject separators, NUL, and portable reserved names before
creating entries. Name collisions are retried without a hidden maximum unless
the caller supplies `max_attempts`.

## 19. Structured Errors

`LocalFileError` retains stable `LocalFileErrorKind`, operation, primary and
secondary namespace paths, the PWD snapshot when relevant, and typed/native
sources. Display text is diagnostic and not a branching contract. Dedicated
writer, copy, rename, and persistence failures are recovery objects; callers
must inspect their typed state rather than infer “nothing happened” from `Err`.

## 20. Capabilities, Requirements, and Runtime Facts

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
The deprecated `supports_atomic_temp_persist()` method is a source-compatibility
alias with identical semantics and is not a stronger guarantee.

Capabilities do not prove that a particular mount, network filesystem, cache,
controller, or device persisted data. `Required` atomicity or durability is a
precondition; `Preferred` permits a typed non-atomic/non-durable outcome;
`NotRequired` avoids extra synchronization. Runtime facts that vary by path are
probed against the selected authority and path. Callers must combine the
capability snapshot with the typed outcome of the actual operation.

## 21. Clone, Concurrency, and Threads

Clone snapshots mutable configuration and shares immutable authority. The crate
makes no promise that one instance's mutable configuration can be shared without
caller synchronization. Open readers, writers, walkers, and temporary resources
own their native state and follow their documented Send/Sync properties. Host
PWD remains process-global; Rooted PWD remains instance-local.

## 22. Path and Filename Utilities

`LocalPaths` provides native lexical normalization, containment, relative-path,
and component operations without touching the filesystem. `LocalFileNames`
validates and manipulates individual native components. `LocalPathCodec`
reversibly maps one component to canonical UTF-8 text for provider transport;
decode rejects non-canonical or platform-invalid encodings. These utilities do
not grant filesystem authority.

## 23. Contract with `qubit-fs-local`

The adapter maps logical provider paths to native namespace paths, maps options
and typed outcomes, and exposes provider resources. It must not reimplement
containment, symbolic-link traversal, temporary ownership, or publication
algorithms. Non-exhaustive native variants require a conservative provider
fallback. Release or scheduled compatibility CI must compile and test the
adapter against this crate so variant drift is detected before publication.

## 24. Direct Application Use

Applications choose Host for process-visible paths and Rooted when one opened
directory is the authority boundary. Configure instance defaults once, clone a
configured snapshot when needed, and use explicit options for one-call policy
changes. Recovery-sensitive code retains writer/copy/rename/persist failures and
branches on typed states.

## 25. Internal Component Boundaries

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

## 26. Verification Strategy

Verification follows contracts rather than line count:

- external tests exercise public success, error, policy, ownership, and
  publication states;
- crate-internal tests exercise real private contracts that public APIs cannot
  deterministically construct, without expanding public visibility;
- property tests cover native path round trips and lexical invariants;
- the exact bilingual README and user-guide Rust fences are included as
  doctests, preventing copied-example drift;
- benchmarks represent codec, walk, handle-budget, copy, writer, Rooted writer,
  and prefix-read workloads;
- bounded fuzz targets exercise codec, path, Host lifecycle, and Rooted
  lifecycle invariants; lifecycle targets use unique per-process sandboxes,
  bounded collision retries, and RAII cleanup, and never rely on ambient
  machine paths;
- Linux, Windows, and macOS are runtime-tested; FreeBSD and Android are
  compile-checked;
- a scheduled and manually dispatchable compatibility workflow tests
  `qubit-fs-local` and `qubit-mime` against the checked-out revision and their
  complete local path-dependency closure;
- the project-level coverage configuration grants no whole-file exemption;
  state-machine branches are covered through public behavior, crate-private
  contracts, or narrow deterministic fault injection.

## 27. Security Guarantees and Explicit Limits

Rooted operations remain anchored to the opened authority and reject lexical or
followed-link escape. Native names remain lossless. Required guarantees are
validated before destructive work where proof is possible. Structured failures
preserve the strongest known recovery state.

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
the three maintained branches point to the same verified commit after release.

## Appendix: Detailed Normative Clauses

This appendix supplies the detailed rules, API shapes, examples, and verification
matrix that are normative in both language editions. It is part of the design,
not an implementation note.

### Object model, constructors, and state

Conceptually, `LocalFileSystem` consists of an immutable, clone-shared
`Arc<LocalAuthorityCore>` and instance-owned `current_directory`, symlink policy,
and default options for read, write, list, copy, create-directory, delete,
rename, temporary file, and temporary directory operations. The exact source
layout may differ, but these invariants must hold:

- authority and its capability snapshot are immutable;
- PWD and defaults belong to one instance and are copied on clone;
- defaults are never placed in the shared `Arc`;
- mutating a clone never mutates another clone's configuration;
- there is no extra instance-level walk or copy hard limit; and
- a Rooted filesystem has exactly one authority participating in operations.

`host()` neither reads nor caches the process PWD. It remains constructible, and
absolute-path operations remain usable, while the PWD is temporarily unreadable.
Only operations which bind a relative path read it, and failures retain the
actual operation and operand. `rooted(root)` resolves one Host-native constructor
path, opens one authority, then exposes virtual `/` with PWD `/`. Its constructor
path is diagnostic-only after opening. A target without the required rooted
containment primitives returns `Unsupported`; optional publication and durability
capabilities do not prevent construction.

There is intentionally no public builder. Callers configure a constructed
instance through `&mut self` setters. Host `set_current_directory` delegates
directly to `std::env::set_current_dir`; it changes process-global state and does
not pre-read or pre-validate PWD. Rooted first resolves and validates a directory
under its authority, then atomically replaces its virtual PWD. A failed setter,
including an unsupported `FollowAcrossScope` policy, leaves the prior state
unchanged.

### Public API and option selection

The public state-and-fact API consists of `scope`, `current_directory`,
`set_current_directory`, `symlink_policy`, `set_symlink_policy`,
`diagnostic_root`, `capabilities`, `limits`, `limits_at`, and `space_at`.
There is one getter/setter pair for each of the nine defaults. Setters validate
structural, scope, and known capability constraints early, but operation entry
points repeat relevant validation because explicit options and path-dependent
runtime facts can differ.

Every configurable operation has an ordinary entry point and a
`*_with_options` entry point. The former uses its instance default; the latter
uses its supplied complete value:

```text
effective_options = explicit_options.unwrap_or(instance_default_options)
```

No fields are implicitly merged, no instance hard cap is applied over explicit
options, and the smaller of two limits is never silently selected. To alter one
default for one call, callers clone that default and alter the clone explicitly.
`symlink_policy_override: None` means inherit the filesystem policy; it is not a
merge of two option objects. Metadata, capability, and space queries have no
spurious `*_with_options` form.

Initial defaults are: no read-open retry; `CreateNew`, no parent creation,
preferred atomicity, and non-required durability for writing; non-recursive,
inherited-link-policy, fail-fast lists; conflict/type-conflict failure,
no metadata preservation, `Auto` source mode, no parent creation, preferred
atomicity, and non-required durability for copying; non-recursive and
exists-error create/delete; no-overwrite, non-required-durability rename; and
PWD parent, default naming, and no parent creation for temporary resources.

All resource limits use `Option` and initially equal `None`: depth, entries,
seen-name bytes, copied bytes, open directories, deadlines, and temporary-name
attempts. `None` always means that the caller set no budget. A reopen policy
matters only when an open-directory budget exists; the crate must not substitute
a hidden fixed handle threshold. `open_retry_timeout` explicitly authorizes
library retries: `None` and zero perform only the first attempt, while a positive
duration permits retries in that monotonic interval.

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

### Path coordinate system and resolution

Host and Rooted share lexical rules: namespace-absolute paths begin at their
namespace root; relative Host paths begin at an operation-time process-PWD
snapshot; relative Rooted paths begin at the instance virtual PWD; empty input
and `.` mean PWD; and `..` removes one component but cannot cross the namespace
root. Rooted stores a normalized namespace-absolute PWD; Host stores none.

For a Rooted authority opened from `/srv/app`, virtual `/`, `/etc/hosts`, and
`/var/data/a.txt` conceptually denote `/srv/app`, `/srv/app/etc/hosts`, and
`/srv/app/var/data/a.txt`. This is explanatory only: access is handle-relative,
never `diagnostic_root.join(...)`. `/srv/app/log` is still a *virtual* path and
therefore conceptually denotes `/srv/app/srv/app/log`. Windows drive, UNC, and
device prefixes are invalid Rooted syntax.

Resolution captures PWD only for a relative input, starts a component stack at
the namespace root or PWD, adds normal components, ignores `.` and empty
components, and rejects `..` at root before producing a namespace-absolute path.
It uses `std::path::Component` and native `OsStr`, never UTF-8 splitting.
Examples at PWD `/` are `"" -> "/"`, `"a/./b" -> "/a/b"`,
`"a/../b" -> "/b"`, and `".." -> InvalidPath`; at `/work/project`,
`"../../tmp" -> "/tmp"` whereas `"../../../tmp" -> InvalidPath`.

Resolution retains directory intent from trailing separators, trailing `.`, or
other native forms until operation type checking. A writer must not turn
`"missing/"` into a regular file. NUL, invalid prefixes, and values that cannot
be represented losslessly are rejected. Copy and rename capture exactly one PWD
snapshot if either operand is relative; if both Host operands are absolute they
do not read PWD, and any lexical failure yields `Unchanged` without native
mutation.

Virtual `/` is addressable for metadata, list, limits, space, and temporary
parents. It is never a writer target, copy source/destination, delete or rename
operand, or persistence target; violations are `InvalidPath`. Host preserves
native drive and prefix semantics, rejects ambiguous drive-relative forms such
as `C:foo` unless the platform can resolve them unambiguously, and reports UNC or
device support only when its complete authority contract is implemented.

### Links, authority, and platform boundary

`Reject` prohibits traversing a link; observing, deleting, or renaming the link
entry itself is not traversal. `FollowWithinScope` permits traversal only while
the resolved name remains in the filesystem namespace. `FollowAcrossScope` is
Host-only. Rooted defaults to `FollowWithinScope`, Host to `FollowAcrossScope`.
Link targets use the same component rules: relative targets begin at the link's
directory and Rooted absolute targets restart at virtual `/`, never Host `/`.
Cycles return a structured path error and must not be hidden behind a caller-
invisible fixed expansion budget.

Final-link semantics are stable: metadata observes the link itself; a reader
follows an allowed target; `CreateNew` treats it as occupied; `Append` follows
it; `CreateOrReplace` follows and replaces its referent while retaining the
link; delete-file removes the link; delete-directory rejects it as the wrong
type; rename moves the link; copy source copies it; copy destination and temp
persist replace its entry. A platform that cannot provide these semantics must
return `Unsupported` or `RequirementNotMet`.

Unix implementations prefer descriptor-relative `*at` operations, final-link
controls, Linux `openat2` containment resolution where available, and descriptor
metadata. Windows implementations use opened directory handles, handle-relative
resolution, reparse-aware metadata, file/volume identity, and native install
primitives. No missing primitive may be replaced by `canonicalize` plus a string
prefix comparison. Mount boundaries are allowed by default; hard-link aliases
between copy source and target are detected and rejected.

### Operations and recovery contracts

Each operation captures policy, options, and needed PWD; validates capabilities;
normalizes operands; completes provable preflight before destructive I/O; invokes
the Host or Rooted backend; then maps all results and failures back into the
namespace coordinate system. Open resources retain their own authority, path,
options, and PWD snapshots.

All public resource identity paths (`path`, `root`, `source_path`, `target_path`,
and `staging_path`) are namespace-absolute and reusable with their owning
filesystem. A directory entry exposes its namespace-absolute `path`, path
relative to the walker root, optional diagnostic path, and metadata. Temporary
and publication paths never expose an authority-relative private representation.

Metadata observes the final entry itself and returns kind, length, permissions,
reliable platform timestamps, and needed identity information. Absence is a
structured `NotFound`, not `Option`. Readers own their native file, implement
`Read + Seek`, and are not redirected by later rename or PWD changes.
`read_prefix` reads at most the requested byte count and never silently loads a
whole file. Objective path limits are distinct from application budgets; Host
may report `VariesByPath`, while `limits_at` and `space_at` probe the nearest
existing authority location and retain unknown facts as unknown.

`CreateNew` and `CreateOrReplace` stage beside the destination, write and flush
bytes, perform required file synchronization, install through a native rename,
and synchronize the parent when required. A publication completed before parent
sync failure is `Published`, not `NotPublished`. Append writes an existing regular
file directly; it cannot satisfy required atomicity and cannot roll back bytes.
Writer lifecycle is `Open`, `Committed`, or `Aborted`; failure knowledge is
`NotPublished`, `Published`, or `Indeterminate` and is independent of lifecycle.

Walkers are lazy `Iterator<Item = LocalResult<LocalDirectoryEntry>>` values.
They never pre-collect a directory tree, fix creation-time policy and authority,
and offer explicit depth, entry, seen-name-byte, open-directory, and deadline
budgets. `FailFast` stops at the first error; continuation returns each error and
continues only safe branches. Directory identity detects recursive followed-link
cycles while output retains logical link paths. Dropping a walker releases
resources only.

Copy produces `Result<LocalCopyOutcome, LocalCopyFailure>` with destination
state `Unchanged`, `PartiallyPublished`, `Published`, or `Indeterminate`. Failure
retains request and failing operands, partial statistics, and staging context
only when cleanup failed. Preflight covers self-copy, hard-link aliases,
destination-inside-source, kinds, conflicts, requirements, and explicit budgets.
File staging is destination-local; required semantics never silently degrade.
Tree copies preserve partial entry statistics, reject unsupported special files
by default, are not generally transactional, and never mutate the source.

Recursive create and delete are likewise non-transactional. If they have already
changed an entry when they fail, they report `PublicationIncomplete` and the
first unfinished namespace-absolute path; otherwise they retain the original
kind. Create's `exists_ok` accepts only an existing directory. File deletion
removes a non-directory or link; directory deletion is separate and recursion is
explicit. Rename always uses a same-authority native rename rather than a
copy-delete emulation, reports `Unchanged`, `Renamed`, or `Indeterminate`, and
cannot silently downgrade its atomic namespace transition.

Temporary resources are RAII-owned entries. Drop performs best-effort cleanup
only while ownership is proven; an indeterminate resource is never automatically
removed. Creation validates names before I/O, stores creation-time PWD when
needed, and creates a per-resource private sandbox. `keep(self)` installs a
generated sibling and reports sandbox cleanup; `close` closes file I/O without
giving up ownership; `cleanup` has an explicit repeat-call contract. Persist
uses the creation-time PWD for a relative target, retains a typed stage and
`NotPublished` or `Indeterminate` result, and never targets Rooted `/`.
`child(component)` accepts one normal component; `descendant(path)` remains
beneath the temporary directory. Identity checks reject ordinary replacement
before deletion or publication, but identity checking and deletion cannot be
claimed atomic across attacker-controlled concurrent replacement or identity
reuse.

### Errors, capabilities, concurrency, and verification

`LocalFileError` stably exposes its kind, public operation, primary/target paths,
PWD context, reason, typed source, and cleanup error. Lexical failures preserve
the caller spelling and PWD snapshot; successful-resolution I/O errors use
namespace-absolute paths. Kinds include invalid path/options/state, missing and
existing entries, directory/type conflicts, permission, unsupported and unmet
requirements, explicit or OS resource limits, corruption, incomplete
publication, indeterminate state, and ordinary I/O. Display text is diagnostic
only. Dedicated copy, rename, commit, and persistence failures are recovery
objects; callers branch on their typed state.

Capabilities report implemented protocols for rooted operations, atomic rename
and replacement, attempting atomic no-replace temporary persistence, durable
rename/copy/write/temporary-file persistence, and do not promise a particular
mount or device outcome.
`can_attempt_atomic_temp_persist()` describes an attempt protocol only; the
deprecated `supports_atomic_temp_persist()` is a source-compatible alias.
`Required` rejects unavailable guarantees before mutation where possible;
`Preferred` may safely downgrade but reports the outcome; `NotRequired` imposes
no guarantee. Unix limits are bytes and Windows limits are UTF-16 code units;
they are never converted into guessed UTF-8 byte limits.

Cloning shares immutable authority, copies Rooted PWD, policy, and all defaults,
and leaves Host clones observing global process PWD. Setters use `&mut self`;
operations use `&self`; the crate provides no internal synchronization for
concurrent configuration. Caller-owned locking is required for one mutable
instance. Readers, writers, walkers, and temporary resources remain valid after
the originating filesystem is reconfigured or dropped because they retain their
creation-time state.

`LocalPaths` converts namespace-absolute native paths to and from canonical
components without PWD or I/O; empty Rooted components mean virtual `/`.
`LocalFileNames` validates native components, applies explicit portable policy
and optional component limits, and creates safe random names without pretending
that every filesystem has a 255-unit limit. `LocalPathCodec` reversibly maps one
native component to canonical UTF-8, rejects aliases and malformed/lowercase or
unnecessary escapes, and has no lossy fallback on Unix or Windows.

`qubit-fs-local` maps abstract requests and canonical paths into this crate's
virtual paths and complete native options, then maps native typed outcomes back.
It does not strip a Rooted leading slash, build another root-relative coordinate
system, duplicate codecs or containment, or retain duplicate defaults. Provider
identity, URI, registry, user metadata, and remote capability policy stay above
this crate.

Verification covers Host/Rooted API symmetry; complete options replacement;
transactional setters; table/property path normalization; native path round
trips; root authority persistence through diagnostic rename; link escape and
cycles; clone/open-resource lifecycles; every writer/copy/rename/temp recovery
state; explicit resource budgets and their absence; virtual-root operation
rules; and downstream typed-state preservation. Linux, Windows, and macOS run
behavioral tests; Android and FreeBSD are compile-checked only. CI must run
all-feature tests and strict Clippy, downstream `qubit-fs-local` contracts and
`qubit-mime` temporary-resource integration, and compile README/user-guide
examples. Fuzz targets use process-unique bounded sandboxes, bounded collision
retry, and RAII cleanup; coverage exempts no whole file.
