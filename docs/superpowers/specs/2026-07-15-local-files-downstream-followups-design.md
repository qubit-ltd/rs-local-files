# rs-local-files Downstream Follow-ups Design

## Goal

Implement the six follow-ups approved after reviewing `rs-local-files` and its
only direct in-tree production consumer, `rs-mime`, without broadening the
crate into an async or virtual-filesystem abstraction.

## Atomic-write durability

Atomic writes record which destination parent directories they create. After
the temporary file is synchronized and renamed, the implementation
synchronizes the destination's immediate parent and then every parent that
contains a newly created directory entry, from deepest to shallowest. Existing
callers of the general parent-creation helper keep their current behavior.

## Temporary-directory child paths

`LocalTempDir::child_path` remains a lexical path join. Its Rustdoc and both
language guides explicitly state that it does not inspect existing symbolic
links and cannot establish filesystem containment. The open/ensure child
helpers retain their observed-symlink checks and TOCTOU disclaimer.

## Recursive-copy cleanup diagnostics

`LocalCopyDirError` records an optional staging path and an optional secondary
cleanup error. The original copy or commit error remains the primary source.
If a skipped staging file cannot be removed, cleanup itself becomes the primary
failure. `StagedFile` also logs cleanup failures reached only through `Drop`.

## Permission contract

The public API and guides document that, on Unix, newly created or replaced
copy files and new atomic-write destinations use mode `0o600`, while newly
created copy directories use `0o700`, subject to a more restrictive umask.
Existing atomic-write destination permissions and opt-in copied permissions
retain their current behavior.

## API evolution

Reader, writer, atomic-stage, and copy-stage enums become non-exhaustive. This
is the smallest source-breaking hardening that permits future variants while
preserving existing variant access. No opaque wrapper types or new source files
are introduced in this change.

## Recursive-copy maintainability

The regular-file copy pipeline is split into destination inspection, staging,
and commit helpers without changing the public API. Existing uncommitted work
has already replaced staging-name polling with Linux file-lease coordination;
that deterministic synchronization is preserved.

## Verification

Behavior changes follow RED-GREEN tests. Final validation runs `./align-ci.sh`
before `./ci-check.sh`; `./coverage.sh json` runs only if CI reports a coverage
threshold failure. The local path consumer in `rs-mime` is tested separately.
