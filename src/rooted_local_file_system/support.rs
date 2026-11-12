// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted support operations.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Component;
#[cfg(windows)]
use std::path::Prefix;
use std::time::Instant;

use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::PathBuf;
use super::internal::symlink_identity::SymlinkIdentity;
use super::io;
use super::resolution_step::ResolutionStep;
use super::root_parent_behavior::RootParentBehavior;
use super::rooted_io_error;
use super::rooted_path;
use crate::directory::LocalResolutionOrigin;
use crate::directory::LocalResolutionScope;
use crate::resolution::native_tokens;
use crate::resolution::native_tokens::NativeToken;
use crate::resolution::session::ResolutionSession;

/// Validates that the requested rooted listing start exists as a directory.
/// Applies `symlink_policy` to the start path; empty input selects the opened
/// root. Returns a type conflict for a non-directory, or a path/policy/native
/// inspection error with listing context.
///
/// # Parameters
///
/// `root` supplies the rooted authority, `path` selects the start, and
/// `symlink_policy` controls link traversal. `deadline` bounds inspection.
///
/// # Returns
///
/// The validated start directory as a root-relative path.
///
/// # Errors
///
/// Returns listing-context errors when the deadline expires, inspection fails,
/// or the requested start is not a directory.
pub(crate) fn validate_rooted_list_start(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    deadline: Option<Instant>,
) -> LocalResult<PathBuf> {
    check_resolution_deadline(deadline, path, LocalFileOperation::List)?;
    if path.as_os_str().is_empty() {
        let metadata_result = root.metadata();
        check_resolution_deadline(deadline, path, LocalFileOperation::List)?;
        let metadata = match metadata_result {
            Ok(metadata) => metadata,
            Err(error) => return Err(rooted_io_error(LocalFileOperation::List, path, error)),
        };
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::List)
                    .with_path(path.to_path_buf()),
            );
        }
        check_resolution_deadline(deadline, path, LocalFileOperation::List)?;
        return Ok(PathBuf::new());
    }
    let original = path;
    let path = resolve_rooted_path_allow_root_with_deadline(
        root,
        path,
        symlink_policy,
        true,
        LocalFileOperation::List,
        deadline,
    )?;
    check_resolution_deadline(deadline, original, LocalFileOperation::List)?;
    let metadata = if path.as_os_str().is_empty() {
        let metadata_result = root.metadata();
        check_resolution_deadline(deadline, original, LocalFileOperation::List)?;
        match metadata_result {
            Ok(metadata) => metadata,
            Err(error) => {
                return Err(rooted_io_error(LocalFileOperation::List, path.as_path(), error));
            }
        }
    } else {
        let relative = match crate::local::LocalRelativePath::new(&path) {
            Ok(relative) => relative,
            Err(error) => return Err(rooted_io_error(LocalFileOperation::List, &path, error)),
        };
        let metadata_result = root.symlink_metadata(&relative);
        check_resolution_deadline(deadline, original, LocalFileOperation::List)?;
        match metadata_result {
            Ok(metadata) => metadata,
            Err(error) => return Err(rooted_io_error(LocalFileOperation::List, &path, error)),
        }
    };
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(
            LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::List)
                .with_path(path.as_path().to_path_buf()),
        );
    }
    check_resolution_deadline(deadline, original, LocalFileOperation::List)?;
    Ok(path)
}

/// Resolves rooted path components while preserving final-entry semantics.
/// Uses [`resolve_rooted_path_allow_root`]'s policy and errors, but rejects an
/// empty virtual-root result because the caller needs a non-empty descendant.
///
/// # Parameters
///
/// `root`, `path`, `symlink_policy`, `follow_final`, and `operation` define
/// the authority, input, traversal policy, final-link behavior, and error
/// context.
///
/// # Returns
///
/// The resolved non-empty path relative to `root`.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, or native inspection errors with
/// `operation` context.
#[inline]
pub(crate) fn resolve_rooted_path(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    resolve_rooted_path_with_deadline(root, path, symlink_policy, follow_final, operation, None)
}

/// Resolves a non-root descendant while checking an optional absolute deadline.
///
/// # Parameters
///
/// `root`, `path`, `symlink_policy`, `follow_final`, and `operation` define
/// the resolution request; `deadline` optionally bounds its execution.
///
/// # Returns
///
/// The resolved non-empty path relative to `root`.
///
/// # Errors
///
/// Returns rooted path, symlink-policy, resource-limit, or native inspection
/// errors.
pub(crate) fn resolve_rooted_path_with_deadline(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    deadline: Option<Instant>,
) -> LocalResult<crate::local::LocalRelativePath> {
    let resolved =
        resolve_rooted_path_allow_root_with_deadline(root, path, symlink_policy, follow_final, operation, deadline)?;
    match crate::local::LocalRelativePath::new(&resolved) {
        Ok(path) => Ok(path),
        Err(error) => Err(rooted_io_error(operation, path, error).with_kind(LocalFileErrorKind::InvalidPath)),
    }
}

/// Resolves a rooted path while allowing the virtual root as the result.
///
/// `follow_final` selects whether the final component participates in link
/// resolution; intermediate links always obey `symlink_policy`. Missing
/// components are retained for later creation. Returns path-validation,
/// forbidden-policy, cycle/escape, or native inspection/read-link errors with
/// `operation` context. `FollowAcrossScope` is invalid for this authority.
///
/// # Parameters
///
/// `root`, `path`, `symlink_policy`, `follow_final`, and `operation` define
/// the resolution request.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, or native inspection errors with
/// `operation` context.
#[inline]
pub(crate) fn resolve_rooted_path_allow_root(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_deadline(root, path, symlink_policy, follow_final, operation, None)
}

/// Resolves a rooted path while checking an optional absolute listing deadline.
///
/// # Parameters
///
/// The arguments define the rooted resolution request; `deadline` is an
/// optional absolute deadline.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, deadline, or native inspection
/// errors.
#[inline]
pub(crate) fn resolve_rooted_path_allow_root_with_deadline(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    deadline: Option<Instant>,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_scope_and_deadline(
        root,
        path,
        symlink_policy,
        follow_final,
        operation,
        LocalResolutionScope::Descendant,
        deadline,
    )
}

/// Resolves a rooted path under an explicit resolution scope and deadline.
///
/// # Parameters
///
/// The arguments define the authority, path, symlink behavior, operation
/// context, resolution scope, and optional absolute deadline.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, deadline, or native inspection
/// errors.
pub(super) fn resolve_rooted_path_allow_root_with_scope_and_deadline(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    scope: LocalResolutionScope,
    deadline: Option<Instant>,
) -> LocalResult<PathBuf> {
    check_resolution_deadline(deadline, path, operation)?;
    if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("FollowAcrossScope is incompatible with a Rooted filesystem")
            .with_path(path.to_path_buf()));
    }
    if path.as_os_str().is_empty() {
        return Ok(PathBuf::new());
    }
    if has_only_normal_components(path) {
        let relative = rooted_path(path, operation)?;
        let session = ResolutionSession::new(root.resolution_contract(), scope);
        if let Some(resolved) =
            try_resolve_without_symlinks(root, &relative, follow_final, deadline, path, operation, &session)?
        {
            return Ok(resolved);
        }
        return resolve_rooted_symlinks(
            root,
            relative.as_path(),
            symlink_policy,
            follow_final,
            operation,
            path,
            deadline,
            scope,
            None,
            None,
            session,
            RootParentBehavior::RejectEscape,
        );
    }
    resolve_rooted_symlinks(
        root,
        path,
        symlink_policy,
        follow_final,
        operation,
        path,
        deadline,
        scope,
        None,
        None,
        ResolutionSession::new(root.resolution_contract(), scope),
        RootParentBehavior::RejectEscape,
    )
}

/// Reports whether a path contains no current-directory or parent components.
///
/// # Parameters
///
/// `path` is the path whose components are inspected.
///
/// # Returns
///
/// `true` when every component is a prefix, root, or normal name.
#[must_use]
#[inline]
fn has_only_normal_components(path: &Path) -> bool {
    path.components().all(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::Normal(_)
        )
    })
}

/// Resolves a rooted path while applying component policy and an optional
/// budget.
///
/// # Parameters
///
/// The arguments provide the rooted request, component policy, deadline, and
/// optional mutable path-binding budget.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, deadline, or native inspection
/// errors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_rooted_path_allow_root_with_component_policy_and_budget(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    component_policy: &crate::directory::LocalComponentPolicy,
    deadline: Option<Instant>,
    budget: Option<&mut crate::directory::binding::PathBindingBudget>,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_component_policy_and_budget_and_parent_behavior(
        root,
        path,
        symlink_policy,
        follow_final,
        operation,
        component_policy,
        deadline,
        budget,
        RootParentBehavior::RejectEscape,
    )
}

/// Resolves a host-root path while applying component policy and an optional
/// budget.
///
/// # Parameters
///
/// The arguments provide the rooted request, component policy, deadline, and
/// optional mutable path-binding budget.
///
/// # Returns
///
/// The resolved host-root path, which may be empty when resolution reaches its
/// root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, deadline, or native inspection
/// errors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_host_root_path_with_component_policy_and_budget(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    component_policy: &crate::directory::LocalComponentPolicy,
    deadline: Option<Instant>,
    budget: Option<&mut crate::directory::binding::PathBindingBudget>,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_component_policy_and_budget_and_parent_behavior(
        root,
        path,
        symlink_policy,
        follow_final,
        operation,
        component_policy,
        deadline,
        budget,
        RootParentBehavior::StayAtHostRoot,
    )
}

/// Resolves a rooted path using the selected behavior at the parent boundary.
///
/// # Parameters
///
/// The arguments provide the resolution request, policy, deadline, budget,
/// and parent-boundary behavior.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns validation, policy, resource-limit, deadline, or native inspection
/// errors.
#[allow(clippy::too_many_arguments)]
fn resolve_rooted_path_allow_root_with_component_policy_and_budget_and_parent_behavior(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    component_policy: &crate::directory::LocalComponentPolicy,
    deadline: Option<Instant>,
    budget: Option<&mut crate::directory::binding::PathBindingBudget>,
    parent_behavior: RootParentBehavior,
) -> LocalResult<PathBuf> {
    check_resolution_deadline(deadline, path, operation)?;
    if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation).with_path(path.to_path_buf()));
    }
    resolve_rooted_symlinks(
        root,
        path,
        symlink_policy,
        follow_final,
        operation,
        path,
        deadline,
        LocalResolutionScope::Descendant,
        Some(component_policy),
        budget,
        ResolutionSession::new(root.resolution_contract(), LocalResolutionScope::Descendant),
        parent_behavior,
    )
}

/// Returns a timeout error when the optional absolute deadline has elapsed.
///
/// # Parameters
///
/// `deadline` is the optional cutoff; `original` and `operation` are retained
/// as error context.
///
/// # Returns
///
/// `Ok(())` while before the deadline.
///
/// # Errors
///
/// Returns a `TimedOut` error when the absolute deadline has elapsed.
pub(super) fn check_resolution_deadline(
    deadline: Option<Instant>,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    let Some(deadline) = deadline else {
        return Ok(());
    };
    #[cfg(test)]
    let now = crate::tests::rooted::support::resolution_observation_tests::resolution_now(Some(deadline));
    #[cfg(not(test))]
    let now = Instant::now();
    if now >= deadline {
        return Err(LocalFileError::from_io(
            operation,
            Some(original.to_path_buf()),
            None,
            io::Error::new(io::ErrorKind::TimedOut, "local listing deadline exceeded"),
        ));
    }
    Ok(())
}

/// Resolves a normal path using descriptor-relative traversal when possible.
///
/// Returns `None` when link-aware fallback is required.
///
/// # Parameters
///
/// The arguments provide the authority, validated relative path, final-link
/// behavior, deadline, original error context, and shared resolution session.
///
/// # Returns
///
/// `Some(path)` when descriptor traversal resolves it, or `None` when fallback
/// is needed.
///
/// # Errors
///
/// Returns validation, resource-limit, deadline, or native traversal errors.
fn try_resolve_without_symlinks(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
    follow_final: bool,
    deadline: Option<Instant>,
    original: &Path,
    operation: LocalFileOperation,
    session: &ResolutionSession,
) -> LocalResult<Option<PathBuf>> {
    check_resolution_deadline(deadline, original, operation)?;
    let (pending_components, pending_bytes) = crate::resolution::native_tokens::measure_path(path.as_path())
        .ok_or_else(|| {
            crate::resolution::session::overflow_error(crate::error::LocalResourceKind::PendingResolutionBytes)
                .with_operation(operation)
                .with_path(original.to_path_buf())
        })?;
    session
        .check_pending(pending_components, pending_bytes)
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    let mut cursor = root
        .resolution_cursor()
        .map_err(|error| rooted_io_error(operation, original, error))?;
    let mut components = path.as_path().components().peekable();
    let mut resolved_components = 0usize;
    let mut resolved_bytes = 0usize;
    while let Some(component) = components.next() {
        check_resolution_deadline(deadline, original, operation)?;
        let name = component.as_os_str();
        session
            .validate_component(name, LocalResolutionOrigin::Input)
            .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
        let (_, component_bytes) =
            crate::resolution::native_tokens::measure_path(Path::new(name)).ok_or_else(|| {
                crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
                    .with_operation(operation)
                    .with_path(original.to_path_buf())
            })?;
        resolved_components = resolved_components.checked_add(1).ok_or_else(|| {
            crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedComponents)
                .with_operation(operation)
                .with_path(original.to_path_buf())
        })?;
        resolved_bytes = resolved_bytes.checked_add(component_bytes).ok_or_else(|| {
            crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
                .with_operation(operation)
                .with_path(original.to_path_buf())
        })?;
        session
            .check_resolved(resolved_components, resolved_bytes)
            .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
        let final_component = components.peek().is_none();
        if final_component && !follow_final {
            return Ok(Some(path.as_path().to_path_buf()));
        }
        let metadata_result = cursor.metadata(name);
        check_resolution_deadline(deadline, original, operation)?;
        let metadata = match metadata_result {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(rooted_io_error(operation, original, error)),
        };
        if metadata.kind() == crate::rooted::EntryKind::Symlink {
            return Ok(None);
        }
        if !final_component {
            if metadata.kind() != crate::rooted::EntryKind::Directory {
                return Ok(None);
            }
            let descend_result = cursor.descend(name);
            check_resolution_deadline(deadline, original, operation)?;
            descend_result.map_err(|error| rooted_io_error(operation, original, error))?;
        }
    }
    Ok(Some(path.as_path().to_path_buf()))
}

/// Resolves path steps while expanding symbolic links under the rooted
/// authority.
///
/// Missing components remain lexical, while rejected traversal and native
/// failures retain the original request context.
/// Expands links from the retained root handle without consulting its
/// diagnostic path or imposing a fixed expansion-count budget. Tracks link
/// identities only during active expansion to reject cycles. Rejected policy,
/// virtual-root escape, invalid target syntax, and native inspection/read-link
/// failures retain `original` and `operation` context.
///
/// # Parameters
///
/// The arguments provide the resolution request, policies, optional budget,
/// session state, and parent-boundary behavior.
///
/// # Returns
///
/// The resolved path, which may represent the virtual root.
///
/// # Errors
///
/// Returns path, policy, resource-limit, deadline, or native inspection errors.
#[allow(clippy::too_many_arguments)]
fn resolve_rooted_symlinks(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    original: &Path,
    deadline: Option<Instant>,
    scope: LocalResolutionScope,
    component_policy: Option<&crate::directory::LocalComponentPolicy>,
    mut budget: Option<&mut crate::directory::binding::PathBindingBudget>,
    mut session: ResolutionSession,
    parent_behavior: RootParentBehavior,
) -> LocalResult<PathBuf> {
    #[cfg(test)]
    crate::tests::rooted::support::resolution_observation_tests::record_fallback();
    if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("FollowAcrossScope is incompatible with a Rooted filesystem")
            .with_path(original.to_path_buf()));
    }
    let mut pending = steps_from_path(path, LocalResolutionOrigin::Input, &session, operation, original)?;
    let mut pending_actions = pending.iter().filter(|step| !is_end_symlink_step(step)).count();
    let mut resolved = Vec::<OsString>::new();
    let mut resolved_component_bytes = Vec::<usize>::new();
    let mut resolved_bytes = 0usize;
    let mut cursor: Option<crate::rooted::RootedResolutionCursor> = None;
    let mut active_symlinks = HashSet::<SymlinkIdentity>::new();
    while let Some(step) = pending.pop_front() {
        check_resolution_deadline(deadline, original, operation)?;
        if !is_end_symlink_step(&step) {
            pending_actions = pending_actions.checked_sub(1).ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(original.to_path_buf())
            })?;
        }
        match step {
            ResolutionStep::ResetRoot => {
                resolved.clear();
                resolved_component_bytes.clear();
                resolved_bytes = 0;
                cursor = None;
            }
            ResolutionStep::Current => ensure_resolved_prefix_is_directory(root, &resolved, original, operation)?,
            ResolutionStep::RequireDirectory => {
                ensure_resolved_prefix_is_directory(root, &resolved, original, operation)?;
            }
            ResolutionStep::Parent => {
                ensure_resolved_prefix_is_directory(root, &resolved, original, operation)?;
                if resolved.is_empty() {
                    if parent_behavior == RootParentBehavior::StayAtHostRoot {
                        cursor = None;
                        continue;
                    }
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("symbolic-link target escaped the Rooted virtual root")
                        .with_path(original.to_path_buf()));
                }
                resolved.pop();
                let removed_bytes = resolved_component_bytes.pop().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(original.to_path_buf())
                })?;
                resolved_bytes = resolved_bytes.checked_sub(removed_bytes).ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(original.to_path_buf())
                })?;
                session
                    .check_resolved(resolved.len(), resolved_bytes)
                    .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                cursor = None;
            }
            ResolutionStep::EndSymlink(identity) => {
                active_symlinks.remove(&identity);
            }
            ResolutionStep::Normal(component, origin) => {
                let (component_bytes, next_resolved_bytes) = validate_resolved_component(
                    &component,
                    origin,
                    (resolved.len(), resolved_bytes),
                    (component_policy, budget.as_deref_mut(), &session, original, operation),
                )?;
                let is_final = pending_actions == 0;
                if is_final && !follow_final {
                    resolved.push(component);
                    resolved_component_bytes.push(component_bytes);
                    resolved_bytes = next_resolved_bytes;
                    cursor = None;
                    continue;
                }
                check_resolution_deadline(deadline, original, operation)?;
                if cursor.is_none() {
                    cursor = open_resolved_cursor(root, &resolved, deadline, original, operation)?;
                }
                let metadata_result = match cursor.as_ref() {
                    Some(cursor) => cursor.metadata(&component),
                    None => {
                        let candidate = relative_from_components(
                            resolved.iter().chain(std::iter::once(&component)),
                            original,
                            operation,
                        )?;
                        root.symlink_metadata(&candidate)
                    }
                };
                check_resolution_deadline(deadline, original, operation)?;
                let metadata = match metadata_result {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        resolved.push(component);
                        resolved_component_bytes.push(component_bytes);
                        resolved_bytes = next_resolved_bytes;
                        cursor = None;
                        continue;
                    }
                    Err(error) => return Err(rooted_io_error(operation, original, error)),
                };
                if metadata.kind() != crate::rooted::EntryKind::Symlink {
                    if !is_final && metadata.kind() == crate::rooted::EntryKind::Directory {
                        let descend_result = cursor.as_mut().map(|cursor| cursor.descend(&component));
                        check_resolution_deadline(deadline, original, operation)?;
                        match descend_result {
                            Some(Ok(())) | None => {}
                            Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => cursor = None,
                            Some(Err(error)) => {
                                return Err(rooted_io_error(operation, original, error));
                            }
                        }
                    } else {
                        cursor = None;
                    }
                    resolved.push(component);
                    resolved_component_bytes.push(component_bytes);
                    resolved_bytes = next_resolved_bytes;
                    continue;
                }
                cursor = None;
                let candidate =
                    relative_from_components(resolved.iter().chain(std::iter::once(&component)), original, operation)?;
                if symlink_policy == LocalSymlinkPolicy::Reject {
                    return Err(LocalFileError::new(LocalFileErrorKind::Unsupported, operation)
                        .with_reason("symbolic-link traversal is rejected by policy")
                        .with_path(original.to_path_buf()));
                }
                let replacement = expand_rooted_symlink(
                    root,
                    &candidate,
                    metadata,
                    scope,
                    deadline,
                    original,
                    operation,
                    &mut active_symlinks,
                    &mut session,
                    pending,
                )?;
                pending_actions = replacement.iter().filter(|step| !is_end_symlink_step(step)).count();
                pending = replacement;
            }
        }
    }
    Ok(path_from_components(resolved.iter()))
}

/// Reads and validates one rooted link target before preparing its replacement
/// steps.
///
/// The identity enters `active_symlinks` before native target inspection and is
/// removed later by its queued end marker. Session accounting and deadline
/// checks remain in the same order as the resolver's traversal phase.
///
/// # Parameters
///
/// `root`, `candidate`, and `metadata` identify the retained link; policy,
/// scope, and deadline constrain expansion. `active_symlinks`, `session`, and
/// `pending` carry traversal state, while `original` and `operation` preserve
/// error context.
///
/// # Returns
///
/// The validated link target steps followed by the pending caller steps.
///
/// # Errors
///
/// Returns cycle, target-validation, resource-limit, deadline, or native
/// read-link errors.
#[allow(clippy::too_many_arguments)]
fn expand_rooted_symlink(
    root: &crate::rooted::Root,
    candidate: &crate::local::LocalRelativePath,
    metadata: crate::rooted::Metadata,
    scope: LocalResolutionScope,
    deadline: Option<Instant>,
    original: &Path,
    operation: LocalFileOperation,
    active_symlinks: &mut HashSet<SymlinkIdentity>,
    session: &mut ResolutionSession,
    pending: VecDeque<ResolutionStep>,
) -> LocalResult<VecDeque<ResolutionStep>> {
    let identity = match metadata.native_identity() {
        Some((device, file)) => SymlinkIdentity::Native(device, file, candidate.as_path().to_path_buf()),
        None => SymlinkIdentity::NamespacePath(candidate.as_path().to_path_buf()),
    };
    if !active_symlinks.insert(identity.clone()) {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
            .with_reason("symbolic-link expansion cycle detected")
            .with_path(original.to_path_buf()));
    }
    session
        .before_follow_link()
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    check_resolution_deadline(deadline, original, operation)?;
    let target_result = root.read_link_limited(
        candidate,
        root.resolution_contract().limits().symlink_target_bytes(),
        |target| {
            session
                .validate_target(target)
                .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))
        },
    );
    check_resolution_deadline(deadline, original, operation)?;
    let target = match target_result {
        Ok(target) => target,
        Err(error) => {
            return Err(error.with_operation(operation).with_path(original.to_path_buf()));
        }
    };
    if scope == LocalResolutionScope::OpeningRoot && !opening_root_target_matches_anchor(root, &target) {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
            .with_reason("absolute symbolic-link target uses a different native root")
            .with_path(original.to_path_buf()));
    }
    replacement_steps_after_link(&target, identity, pending, session, operation, original)
}

/// Prepends a validated link target and checks the resulting pending-work
/// budget.
///
/// The active-link marker closes after the target's steps, before the caller's
/// previously pending steps. The complete queue is measured before traversal
/// resumes so link expansion keeps the same resource-limit boundary.
///
/// # Parameters
///
/// `target` supplies the link target, `identity` tracks active expansion,
/// `pending` contains the caller's remaining steps, and `session` enforces
/// resolution limits. `operation` and `original` preserve error context.
///
/// # Returns
///
/// The validated target steps, link-closing marker, and remaining caller steps.
///
/// # Errors
///
/// Returns path-validation or pending-resolution resource-limit errors with
/// the caller's operation and original path.
fn replacement_steps_after_link(
    target: &Path,
    identity: SymlinkIdentity,
    pending: VecDeque<ResolutionStep>,
    session: &ResolutionSession,
    operation: LocalFileOperation,
    original: &Path,
) -> LocalResult<VecDeque<ResolutionStep>> {
    let mut replacement = steps_from_path(
        target,
        LocalResolutionOrigin::SymlinkTarget,
        session,
        operation,
        original,
    )?;
    replacement.push_back(ResolutionStep::EndSymlink(identity));
    replacement.extend(pending);
    let (pending_count, pending_bytes) = pending_measure(&replacement).ok_or_else(|| {
        crate::resolution::session::overflow_error(crate::error::LocalResourceKind::PendingResolutionBytes)
            .with_operation(operation)
            .with_path(original.to_path_buf())
    })?;
    session
        .check_pending(pending_count, pending_bytes)
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    Ok(replacement)
}

/// Validates one normal component and accounts for its resource usage.
///
/// # Parameters
///
/// The component and origin identify the step; `resolved_components` and
/// `resolved_bytes` describe retained state. Policy, budget, session, and
/// request context apply their respective limits and error attribution.
///
/// # Returns
///
/// The component's encoded byte count and the resulting resolved byte count.
///
/// # Errors
///
/// Returns component-policy, budget, or resource-limit errors.
fn validate_resolved_component(
    component: &OsStr,
    origin: LocalResolutionOrigin,
    resolved: (usize, usize),
    context: (
        Option<&crate::directory::LocalComponentPolicy>,
        Option<&mut crate::directory::binding::PathBindingBudget>,
        &ResolutionSession,
        &Path,
        LocalFileOperation,
    ),
) -> LocalResult<(usize, usize)> {
    let (resolved_components, resolved_bytes) = resolved;
    let (component_policy, budget, session, original, operation) = context;
    let component_bytes = native_path_bytes(Path::new(component))
        .and_then(|bytes| bytes.checked_add(native_unit_bytes()))
        .ok_or_else(|| {
            crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
                .with_operation(operation)
                .with_path(original.to_path_buf())
        })?;
    if let Some(budget) = budget {
        budget.debit(component_bytes, operation, original)?;
    }
    if let Some(component_policy) = component_policy {
        component_policy
            .validate_component(component)
            .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    }
    session
        .validate_component(component, origin)
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    let next_resolved_bytes = resolved_bytes.checked_add(component_bytes).ok_or_else(|| {
        crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
            .with_operation(operation)
            .with_path(original.to_path_buf())
    })?;
    session
        .check_resolved(
            resolved_components.checked_add(1).ok_or_else(|| {
                crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedComponents)
                    .with_operation(operation)
                    .with_path(original.to_path_buf())
            })?,
            next_resolved_bytes,
        )
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
    Ok((component_bytes, next_resolved_bytes))
}

/// Checks whether an absolute link target names the opened root's native
/// anchor.
///
/// # Parameters
///
/// `root` supplies the opened authority and `target` is the absolute link
/// target.
///
/// # Returns
///
/// `true` when the target's native root anchor matches the opened root.
fn opening_root_target_matches_anchor(root: &crate::rooted::Root, target: &Path) -> bool {
    #[cfg(windows)]
    {
        let Some(target_prefix) = target.components().find_map(|component| match component {
            Component::Prefix(prefix) => Some((prefix.kind(), prefix.as_os_str())),
            _ => None,
        }) else {
            return true;
        };
        let Some(root_prefix) = root.path().components().find_map(|component| match component {
            Component::Prefix(prefix) => Some((prefix.kind(), prefix.as_os_str())),
            _ => None,
        }) else {
            return false;
        };
        match (target_prefix, root_prefix) {
            ((Prefix::Disk(target_drive), _), (Prefix::Disk(root_drive), _))
            | ((Prefix::Disk(target_drive), _), (Prefix::VerbatimDisk(root_drive), _))
            | ((Prefix::VerbatimDisk(target_drive), _), (Prefix::Disk(root_drive), _))
            | ((Prefix::VerbatimDisk(target_drive), _), (Prefix::VerbatimDisk(root_drive), _)) => {
                target_drive.eq_ignore_ascii_case(&root_drive)
            }
            ((_, target_prefix), (_, root_prefix)) => target_prefix
                .to_string_lossy()
                .eq_ignore_ascii_case(&root_prefix.to_string_lossy()),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (root, target);
        true
    }
}

/// Ensures the resolved prefix exists and is a directory beneath the root.
///
/// # Parameters
///
/// `root` supplies the authority, `resolved` the prefix components, and
/// `original` plus `operation` preserve request error context.
///
/// # Returns
///
/// Returns `Ok(())` when the prefix is the virtual root or an existing
/// directory.
///
/// # Errors
///
/// Returns native inspection errors or `NotDirectory` when the prefix is not a
/// directory.
fn ensure_resolved_prefix_is_directory(
    root: &crate::rooted::Root,
    resolved: &[OsString],
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    if resolved.is_empty() {
        return Ok(());
    }
    let prefix = relative_from_components(resolved.iter(), original, operation)?;
    let metadata = root
        .symlink_metadata(&prefix)
        .map_err(|error| rooted_io_error(operation, original, error))?;
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(LocalFileError::new(LocalFileErrorKind::NotDirectory, operation).with_path(original.to_path_buf()));
    }
    Ok(())
}

/// Opens a cursor at the resolved directory prefix, returning `None` if absent
/// or if a prefix component is not a directory.
///
/// # Parameters
///
/// `root` supplies the authority, `resolved` selects the prefix, and
/// `deadline`, `original`, and `operation` bound traversal and preserve error
/// context.
///
/// # Returns
///
/// An opened cursor when the prefix exists as a directory, or `None` when
/// absent.
///
/// # Errors
///
/// Returns deadline or native traversal errors.
fn open_resolved_cursor(
    root: &crate::rooted::Root,
    resolved: &[OsString],
    deadline: Option<Instant>,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<Option<crate::rooted::RootedResolutionCursor>> {
    let mut cursor = root
        .resolution_cursor()
        .map_err(|error| rooted_io_error(operation, original, error))?;
    for component in resolved {
        check_resolution_deadline(deadline, original, operation)?;
        let metadata_result = cursor.metadata(component);
        check_resolution_deadline(deadline, original, operation)?;
        let metadata = match metadata_result {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(rooted_io_error(operation, original, error)),
        };
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Ok(None);
        }
        let descend_result = cursor.descend(component);
        check_resolution_deadline(deadline, original, operation)?;
        match descend_result {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(rooted_io_error(operation, original, error)),
        }
    }
    Ok(Some(cursor))
}

/// Measures a path in native encoded bytes, returning `None` on overflow.
///
/// # Parameters
///
/// `path` is measured using the platform's native encoding.
///
/// # Returns
///
/// The native encoded byte count, or `None` when encoding or checked arithmetic
/// fails.
fn native_path_bytes(path: &Path) -> Option<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(path.as_os_str().as_bytes().len())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().count().checked_mul(2)
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_str()?.len().checked_mul(4)
    }
}

/// Measures pending resolution components and native bytes with checked
/// arithmetic.
///
/// # Parameters
///
/// `steps` is the pending resolution queue to measure.
///
/// # Returns
///
/// The pending component count and byte count, or `None` if accounting
/// overflows.
fn pending_measure(steps: &VecDeque<ResolutionStep>) -> Option<(usize, usize)> {
    let mut components = 0usize;
    let mut bytes = 0usize;
    let unit_bytes = native_unit_bytes();
    for step in steps {
        match step {
            ResolutionStep::ResetRoot | ResolutionStep::RequireDirectory | ResolutionStep::EndSymlink(_) => {}
            ResolutionStep::Current => {
                components = components.checked_add(1)?;
                bytes = bytes.checked_add(checked_native_units(2, unit_bytes)?)?;
            }
            ResolutionStep::Parent => {
                components = components.checked_add(1)?;
                bytes = bytes.checked_add(checked_native_units(3, unit_bytes)?)?;
            }
            ResolutionStep::Normal(name, _) => {
                components = components.checked_add(1)?;
                let native = native_path_bytes(Path::new(name))?;
                bytes = bytes.checked_add(native.checked_add(unit_bytes)?)?;
            }
        }
    }
    Some((components, bytes))
}

/// Returns the native byte width of one path separator unit.
///
/// # Returns
///
/// Two bytes on Windows and one byte on other supported platforms.
#[inline]
const fn native_unit_bytes() -> usize {
    if cfg!(windows) { 2 } else { 1 }
}

/// Multiplies native path units by their width without overflowing.
///
/// # Parameters
///
/// `units` is the count and `unit_bytes` is the native byte width per unit.
///
/// # Returns
///
/// The product, or `None` if multiplication overflows.
#[inline]
fn checked_native_units(units: usize, unit_bytes: usize) -> Option<usize> {
    units.checked_mul(unit_bytes)
}

/// Reports whether a pending resolution step only closes an expanded link.
///
/// # Parameters
///
/// `step` is the pending operation to classify.
///
/// # Returns
///
/// `true` when `step` closes an expanded symbolic link.
#[must_use]
#[inline]
const fn is_end_symlink_step(step: &ResolutionStep) -> bool {
    matches!(step, ResolutionStep::EndSymlink(_))
}

/// Converts native path syntax into pending virtual-root resolution steps.
/// Returns `InvalidPath` for native prefixes; roots restart at the virtual
/// root, dots disappear, and parents remain deferred traversal steps.
///
/// # Parameters
///
/// `path`, `origin`, and `session` supply tokens and validation policy;
/// `operation` and `original` preserve the caller's error context.
///
/// # Returns
///
/// The pending virtual-root resolution steps represented by `path`.
///
/// # Errors
///
/// Returns token validation errors with the caller's operation and path
/// context.
fn steps_from_path(
    path: &Path,
    origin: LocalResolutionOrigin,
    session: &ResolutionSession,
    operation: LocalFileOperation,
    original: &Path,
) -> LocalResult<VecDeque<ResolutionStep>> {
    let mut steps = VecDeque::new();
    if path.is_absolute() {
        steps.push_back(ResolutionStep::ResetRoot);
    }
    for token in native_tokens::tokenize(path, origin, session)
        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?
    {
        match token {
            NativeToken::Current(_) => steps.push_back(ResolutionStep::Current),
            NativeToken::Parent(_) => steps.push_back(ResolutionStep::Parent),
            NativeToken::Normal(name, origin) => steps.push_back(ResolutionStep::Normal(name, origin)),
            NativeToken::RequireDirectory => steps.push_back(ResolutionStep::RequireDirectory),
        }
    }
    Ok(steps)
}

/// Builds a non-empty rooted relative path from normalized native components.
/// Returns `InvalidPath` with `original` and `operation` context when the
/// assembled path fails relative-path validation.
///
/// # Type Parameters
///
/// `'component` is the borrow lifetime of the supplied native components.
///
/// # Parameters
///
/// `components` supplies normalized names; `original` and `operation` identify
/// the caller's request in any validation error.
///
/// # Returns
///
/// The validated relative path made from `components`.
///
/// # Errors
///
/// Returns `InvalidPath` with original request context if validation fails.
fn relative_from_components<'component>(
    components: impl Iterator<Item = &'component OsString>,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    match crate::local::LocalRelativePath::new(&path) {
        Ok(path) => Ok(path),
        Err(error) => Err(rooted_io_error(operation, original, error).with_kind(LocalFileErrorKind::InvalidPath)),
    }
}

/// Builds a rooted relative path, including the empty virtual-root spelling.
///
/// # Type Parameters
///
/// `'component` is the borrow lifetime of the supplied native components.
///
/// # Parameters
///
/// `components` supplies the normalized native path components.
///
/// # Returns
///
/// A path containing the supplied components, including an empty path for no
/// components.
fn path_from_components<'component>(components: impl Iterator<Item = &'component OsString>) -> PathBuf {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    path
}

/// Synchronizes ancestors that may have gained newly created directories.
/// Synchronizes the parents of `target`'s non-empty ancestors from deepest to
/// shallowest; the caller synchronizes `target`'s immediate parent separately.
/// Stops and propagates the first native synchronization failure.
///
/// # Parameters
///
/// `root` supplies the rooted authority and `target` is the validated relative
/// path.
///
/// # Returns
///
/// Returns `Ok(())` after syncing every non-empty ancestor parent, or when none
/// exist.
///
/// # Errors
///
/// Returns the first native synchronization error and stops syncing shallower
/// parents.
pub(crate) fn sync_rooted_copy_parent_chain(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
) -> io::Result<()> {
    let mut parent = target.as_path().parent().map(Path::to_path_buf);
    while let Some(path) = parent {
        if path.as_os_str().is_empty() {
            break;
        }
        let path = crate::local::LocalRelativePath::new(&path).expect("parent of a validated rooted path is valid");
        root.sync_parent(&path)?;
        parent = path.as_path().parent().map(Path::to_path_buf);
    }
    Ok(())
}

#[cfg(test)]
mod pending_measure_tests {
    use super::checked_native_units;

    #[test]
    fn test_native_unit_measurement_reports_overflow() {
        assert_eq!(None, checked_native_units(usize::MAX, 2));
    }
}

#[cfg(test)]
mod native_resolution_failure_tests {
    use std::fs;
    use std::path::Path;

    use crate::LocalFileOperation;
    use crate::LocalSymlinkPolicy;
    use crate::directory::LocalResolutionScope;
    use crate::resolution::ResolutionContract;
    use crate::resolution::session::ResolutionSession;
    use crate::rooted::Root;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_fast_resolution_native_failures_retain_original_operation_and_path() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        fs::create_dir(fixture.path().join("a")).expect("directory should exist");
        fs::write(fixture.path().join("a/file"), b"payload").expect("file should exist");
        let root = Root::open(fixture.path()).expect("root authority should open");
        let path = crate::local::LocalRelativePath::new(Path::new("a/file")).expect("input should be relative");
        let original = Path::new("original/request");
        let session = ResolutionSession::new(&ResolutionContract::unrestricted(), LocalResolutionScope::Descendant);
        for point in [
            NativeIoPoint::OpenCursor,
            NativeIoPoint::CursorMetadata,
            NativeIoPoint::CursorDescend,
        ] {
            let guard = NativeIoFaultGuard::fail_on(point, 1);
            let error = super::try_resolve_without_symlinks(
                &root,
                &path,
                true,
                None,
                original,
                LocalFileOperation::List,
                &session,
            )
            .expect_err("native cursor failure must stop resolution");
            assert!(guard.fired(), "the selected native boundary must be reached");
            assert_eq!(error.operation(), LocalFileOperation::List);
            assert_eq!(error.path(), Some(original));
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
            assert_eq!(
                error
                    .io_error()
                    .expect("native failure must remain available")
                    .to_string(),
                "injected native I/O contract failure"
            );
        }
    }

    #[test]
    fn test_fallback_resolution_native_failures_keep_context_across_parent_upwalk() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        fs::create_dir_all(fixture.path().join("a/b")).expect("directories should exist");
        fs::write(fixture.path().join("a/file"), b"payload").expect("file should exist");
        let root = Root::open(fixture.path()).expect("root authority should open");
        let original = Path::new("requested/link");
        let cases = [
            (NativeIoPoint::OpenCursor, 1),
            (NativeIoPoint::CursorMetadata, 1),
            (NativeIoPoint::CursorDescend, 1),
            (NativeIoPoint::OpenCursor, 2),
            (NativeIoPoint::CursorMetadata, 3),
            (NativeIoPoint::CursorDescend, 3),
        ];
        for (point, occurrence) in cases {
            let guard = NativeIoFaultGuard::fail_on(point, occurrence);
            let session = ResolutionSession::new(&ResolutionContract::unrestricted(), LocalResolutionScope::Descendant);
            let error = super::resolve_rooted_symlinks(
                &root,
                Path::new("a/b/../file"),
                LocalSymlinkPolicy::FollowWithinScope,
                true,
                LocalFileOperation::List,
                original,
                None,
                LocalResolutionScope::Descendant,
                None,
                None,
                session,
                super::RootParentBehavior::RejectEscape,
            )
            .expect_err("native failure must stop fallback before further traversal");
            assert!(
                guard.fired(),
                "selected native boundary {point:?}/{occurrence} must be reached"
            );
            assert_eq!(error.operation(), LocalFileOperation::List);
            assert_eq!(error.path(), Some(original));
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
            assert_eq!(
                error
                    .io_error()
                    .expect("native failure must remain available")
                    .to_string(),
                "injected native I/O contract failure"
            );
        }
    }
}
