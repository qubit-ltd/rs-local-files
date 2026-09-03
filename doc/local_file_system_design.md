# Qubit Local Files Complete Filesystem Design

[中文设计文档](local_file_system_design.zh_CN.md) ·
[User guide](user_guide.md) · [README](../README.md)

> Status: normative design
>
> Last updated: 2026-09-03

This document defines the stable design of `qubit-local-files`. Public APIs,
platform implementations, tests, READMEs, and user guides must remain aligned
with it. It describes the intended completed system, not migration history or
temporary implementation details. “Must”, “must not”, and “should” express
normative requirements. Non-semantic modifiers such as a particular `const` or
`inline` annotation are coding-policy details rather than API contracts.

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
- atomic no-replace temporary persistence;
- durable rename;
- durable file copy;
- durable writer publication.

Capabilities do not prove that a particular mount, network filesystem, cache,
controller, or device persisted data. `Required` atomicity or durability is a
precondition; `Preferred` permits a typed non-atomic/non-durable outcome;
`NotRequired` avoids extra synchronization. Runtime facts that vary by path are
probed against the selected authority and path.

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
  lifecycle invariants;
- Linux, Windows, and macOS are runtime-tested; FreeBSD and Android are
  compile-checked;
- release compatibility checks exercise `qubit-fs-local` and `qubit-mime`;
- coverage exemptions are limited to native/platform failures that cannot be
  manufactured deterministically, not whole state machines with testable
  contracts.

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
