// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted support operations.

use std::time::Instant;

use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::PathBuf;
use super::io;
use super::resolution_step::ResolutionStep;
use super::rooted_io_error;
use super::rooted_path;
use super::symlink_identity::SymlinkIdentity;
use crate::directory::{LocalResolutionOrigin, LocalResolutionScope};
use crate::resolution::native_tokens::{self, NativeToken};
use crate::resolution::session::ResolutionSession;

/// Validates that the requested rooted listing start exists as a directory.
/// Applies `symlink_policy` to the start path; empty input selects the opened
/// root. Returns a type conflict for a non-directory, or a path/policy/native
/// inspection error with listing context.
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
pub(crate) fn resolve_rooted_path(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    let resolved = resolve_rooted_path_allow_root(root, path, symlink_policy, follow_final, operation)?;
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
pub(crate) fn resolve_rooted_path_allow_root_with_deadline(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    deadline: Option<Instant>,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_scope_and_deadline(
        root, path, symlink_policy, follow_final, operation, LocalResolutionScope::Descendant, deadline,
    )
}

pub(crate) fn resolve_rooted_path_allow_root_with_scope(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    scope: LocalResolutionScope,
) -> LocalResult<PathBuf> {
    resolve_rooted_path_allow_root_with_scope_and_deadline(
        root, path, symlink_policy, follow_final, operation, scope, None,
    )
}

fn resolve_rooted_path_allow_root_with_scope_and_deadline(
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
        if root.resolution_contract().is_unrestricted()
            && let Some(resolved) = try_resolve_without_symlinks(
                root, &relative, follow_final, deadline, path, operation,
            )?
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
            ResolutionSession::new(root.resolution_contract(), scope),
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
    )
}

fn has_only_normal_components(path: &Path) -> bool {
    use std::path::Component;
    path.components().all(|component| matches!(component, Component::Prefix(_) | Component::RootDir | Component::Normal(_)))
}

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
    )
}

fn check_resolution_deadline(
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

/// Checks a normal rooted path with linear descriptor-relative work.
/// Each intermediate component requires metadata inspection and a directory
/// open; the final component is inspected only when `follow_final` is true.
/// Any uncertainty returns `None`, allowing the symlink-aware resolver to
/// classify the path and its errors.
fn try_resolve_without_symlinks(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
    follow_final: bool,
    deadline: Option<Instant>,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<Option<PathBuf>> {
    check_resolution_deadline(deadline, original, operation)?;
    let mut cursor = root.resolution_cursor().map_err(|error| rooted_io_error(operation, original, error))?;
    let mut components = path.as_path().components().peekable();
    while let Some(component) = components.next() {
        check_resolution_deadline(deadline, original, operation)?;
        let name = component.as_os_str();
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

/// Expands symlinks from the retained root handle without consulting its
/// diagnostic path or imposing a fixed expansion-count budget.
/// Tracks identities only during active expansion to reject cycles. Missing
/// components remain lexical; rejected policy, virtual-root escape, invalid
/// target syntax, and native inspection/read-link failures carry `original`
/// and `operation` context.
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
) -> LocalResult<PathBuf> {
    use std::collections::HashSet;
    #[cfg(test)]
    crate::tests::rooted::support::resolution_observation_tests::record_fallback();
    if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("FollowAcrossScope is incompatible with a Rooted filesystem")
            .with_path(original.to_path_buf()));
    }
    let mut pending = steps_from_path(path, LocalResolutionOrigin::Input, &session)?;
    let mut resolved = Vec::<std::ffi::OsString>::new();
    let mut active_symlinks = HashSet::<SymlinkIdentity>::new();
    while let Some(step) = pending.pop_front() {
        check_resolution_deadline(deadline, original, operation)?;
        match step {
            ResolutionStep::ResetRoot => resolved.clear(),
            ResolutionStep::Current => ensure_resolved_prefix_is_directory(root, &resolved, original, operation)?,
            ResolutionStep::Parent => {
                ensure_resolved_prefix_is_directory(root, &resolved, original, operation)?;
                if resolved.pop().is_none() {
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("symbolic-link target escaped the Rooted virtual root")
                        .with_path(original.to_path_buf()));
                }
                let (components, bytes) = resolved_measure(&resolved).ok_or_else(|| {
                    crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
                        .with_operation(operation).with_path(original.to_path_buf())
                })?;
                session.check_resolved(components, bytes).map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
            }
            ResolutionStep::EndSymlink(identity) => {
                active_symlinks.remove(&identity);
            }
            ResolutionStep::Normal(component, origin) => {
                let component_bytes = component.len().checked_add(1).ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(original.to_path_buf())
                })?;
                if let Some(budget) = budget.as_deref_mut() {
                    budget.debit(component_bytes, operation, original)?;
                }
                if let Some(component_policy) = component_policy {
                    component_policy
                        .validate_component(&component)
                        .map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                }
                session.validate_component(&component, origin).map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                let candidate =
                    relative_from_components(resolved.iter().chain(std::iter::once(&component)), original, operation)?;
                let mut prospective = resolved.clone();
                prospective.push(component.clone());
                let (resolved_count, resolved_bytes) = resolved_measure(&prospective).ok_or_else(|| {
                    crate::resolution::session::overflow_error(crate::error::LocalResourceKind::ResolvedPathBytes)
                        .with_operation(operation).with_path(original.to_path_buf())
                })?;
                session.check_resolved(resolved_count, resolved_bytes).map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                let is_final = pending.iter().all(is_end_symlink_step);
                if is_final && !follow_final {
                    resolved.push(component);
                    continue;
                }
                check_resolution_deadline(deadline, original, operation)?;
                let metadata_result = root.symlink_metadata(&candidate);
                check_resolution_deadline(deadline, original, operation)?;
                let metadata = match metadata_result {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {
                        resolved.push(component);
                        continue;
                    }
                    Err(error) => return Err(rooted_io_error(operation, original, error)),
                };
                if metadata.kind() != crate::rooted::EntryKind::Symlink {
                    resolved.push(component);
                    continue;
                }
                if symlink_policy == LocalSymlinkPolicy::Reject {
                    return Err(LocalFileError::new(LocalFileErrorKind::Unsupported, operation)
                        .with_reason("symbolic-link traversal is rejected by policy")
                        .with_path(original.to_path_buf()));
                }
                let identity = match metadata.native_identity() {
                    Some((device, file)) => SymlinkIdentity::Native(device, file),
                    None => SymlinkIdentity::NamespacePath(candidate.as_path().to_path_buf()),
                };
                if !active_symlinks.insert(identity.clone()) {
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("symbolic-link expansion cycle detected")
                        .with_path(original.to_path_buf()));
                }
                session.before_follow_link().map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                check_resolution_deadline(deadline, original, operation)?;
        let target_result = root.read_link_limited(
            &candidate,
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
                    Err(error) => return Err(error.with_operation(operation).with_path(original.to_path_buf())),
                };
                if scope == LocalResolutionScope::OpeningRoot
                    && !opening_root_target_matches_anchor(root, &target)
                {
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("absolute symbolic-link target uses a different native root")
                        .with_path(original.to_path_buf()));
                }
                let mut replacement = steps_from_path(&target, LocalResolutionOrigin::SymlinkTarget, &session)?;
                replacement.push_back(ResolutionStep::EndSymlink(identity));
                replacement.extend(pending);
                let (pending_count, pending_bytes) = pending_measure(&replacement).ok_or_else(|| {
                    crate::resolution::session::overflow_error(crate::error::LocalResourceKind::PendingResolutionBytes)
                        .with_operation(operation).with_path(original.to_path_buf())
                })?;
                session.check_pending(pending_count, pending_bytes).map_err(|error| error.with_operation(operation).with_path(original.to_path_buf()))?;
                pending = replacement;
            }
        }
    }
    Ok(path_from_components(resolved.iter()))
}

fn opening_root_target_matches_anchor(root: &crate::rooted::Root, target: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::path::Component;
        let Some(target_prefix) = target.components().find_map(|component| match component {
            Component::Prefix(prefix) => Some(prefix.as_os_str()),
            _ => None,
        }) else {
            return true;
        };
        let Some(root_prefix) = root.path().components().find_map(|component| match component {
            Component::Prefix(prefix) => Some(prefix.as_os_str()),
            _ => None,
        }) else {
            return false;
        };
        target_prefix.to_string_lossy().eq_ignore_ascii_case(&root_prefix.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        let _ = (root, target);
        true
    }
}

fn ensure_resolved_prefix_is_directory(
    root: &crate::rooted::Root,
    resolved: &[std::ffi::OsString],
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

fn resolved_measure(components: &[std::ffi::OsString]) -> Option<(usize, usize)> {
    let mut bytes = 0usize;
    for component in components {
        let native = native_path_bytes(Path::new(component))?;
        bytes = bytes.checked_add(native)?.checked_add(if cfg!(windows) { 2 } else { 1 })?;
    }
    Some((components.len(), bytes))
}

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

fn pending_measure(steps: &std::collections::VecDeque<ResolutionStep>) -> Option<(usize, usize)> {
    let mut components = 0usize;
    let mut bytes = 0usize;
    for step in steps {
        match step {
            ResolutionStep::ResetRoot | ResolutionStep::EndSymlink(_) => {}
            ResolutionStep::Current | ResolutionStep::Parent => {
                components = components.checked_add(1)?;
                bytes = bytes.checked_add(if cfg!(windows) { 2 } else { 1 })?;
            }
            ResolutionStep::Normal(name, _) => {
                components = components.checked_add(1)?;
                let native = native_path_bytes(Path::new(name))?;
                bytes = bytes.checked_add(native)?.checked_add(if cfg!(windows) { 2 } else { 1 })?;
            }
        }
    }
    Some((components, bytes))
}

/// Reports whether a pending resolution step only closes an expanded link.
#[must_use]
const fn is_end_symlink_step(step: &ResolutionStep) -> bool {
    matches!(step, ResolutionStep::EndSymlink(_))
}

/// Converts native path syntax into pending virtual-root resolution steps.
/// Returns `InvalidPath` for native prefixes; roots restart at the virtual
/// root, dots disappear, and parents remain deferred traversal steps.
fn steps_from_path(
    path: &Path,
    origin: LocalResolutionOrigin,
    session: &ResolutionSession,
) -> LocalResult<std::collections::VecDeque<ResolutionStep>> {
    use std::collections::VecDeque;

    let mut steps = VecDeque::new();
    if path.is_absolute() {
        steps.push_back(ResolutionStep::ResetRoot);
    }
    for token in native_tokens::tokenize(path, origin, session)? {
        match token {
            NativeToken::Current(_) => steps.push_back(ResolutionStep::Current),
            NativeToken::Parent(_) => steps.push_back(ResolutionStep::Parent),
            NativeToken::Normal(name, origin) => steps.push_back(ResolutionStep::Normal(name, origin)),
        }
    }
    Ok(steps)
}

/// Builds a non-empty rooted relative path from normalized native components.
/// Returns `InvalidPath` with `original` and `operation` context when the
/// assembled path fails relative-path validation.
fn relative_from_components<'component>(
    components: impl Iterator<Item = &'component std::ffi::OsString>,
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
fn path_from_components<'component>(components: impl Iterator<Item = &'component std::ffi::OsString>) -> PathBuf {
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
