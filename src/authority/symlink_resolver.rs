// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Symlink policy validation at an opened namespace boundary.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::RelativePath;
use crate::platform::NamespaceHandle;

/// Resolves a validated descendant according to Rooted symlink policy.
///
/// # Errors
///
/// Returns a bind-path error when inspection fails. Reject mode fails on the
/// first link. Follow modes reject absolute targets, parent escape, and link
/// expansion cycles before returning a handle-contained path.
pub(crate) fn resolve(
    root: &NamespaceHandle,
    path: &RelativePath,
    policy: LocalSymlinkPolicy,
) -> LocalResult<RelativePath> {
    let original = path.as_path();
    let mut pending = path_components(original);
    let mut resolved = Vec::<OsString>::new();
    let mut followed_links = 0_u8;
    while let Some(component) = pending.pop_front() {
        let candidate = relative_from_components(
            resolved.iter().chain(std::iter::once(&component)),
        )?;
        let metadata = match root.metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == LocalFileErrorKind::NotFound => {
                resolved.push(component);
                resolved.extend(pending);
                return relative_from_components(resolved.iter());
            }
            Err(error) => return Err(error),
        };
        if metadata.kind() != LocalFileKind::Symlink {
            resolved.push(component);
            continue;
        }
        if policy == LocalSymlinkPolicy::Reject {
            return Err(symlink_error(
                original,
                LocalFileErrorKind::Unsupported,
                "symbolic-link traversal is rejected by policy",
            ));
        }
        followed_links = followed_links.saturating_add(1);
        if followed_links > 40 {
            return Err(symlink_error(
                original,
                LocalFileErrorKind::InvalidPath,
                "symbolic-link expansion exceeded the cycle limit",
            ));
        }
        let target = root.read_link(&candidate)?;
        let mut replacement = resolved.clone();
        apply_link_target(&mut replacement, &target, original)?;
        replacement.extend(pending);
        pending = replacement.into();
        resolved.clear();
    }
    relative_from_components(resolved.iter())
}

/// Resolves only intermediate components, preserving the final entry.
pub(crate) fn resolve_parent(
    root: &NamespaceHandle,
    path: &RelativePath,
    policy: LocalSymlinkPolicy,
) -> LocalResult<RelativePath> {
    let mut components = path.as_path().components();
    let Some(final_component) = components.next_back() else {
        return Ok(path.clone());
    };
    let parent = components.as_path();
    let resolved_parent = if parent.as_os_str().is_empty() {
        RelativePath::parse(Path::new(""))?
    } else {
        resolve(root, &RelativePath::parse(parent)?, policy)?
    };
    let mut result = resolved_parent.as_path().to_path_buf();
    result.push(final_component.as_os_str());
    RelativePath::parse(&result)
}

/// Collects normal components from an already validated relative path.
fn path_components(path: &Path) -> VecDeque<OsString> {
    path.components()
        .map(|component| component.as_os_str().to_os_string())
        .collect()
}

/// Applies one relative link target to the resolved parent component stack.
fn apply_link_target(
    resolved: &mut Vec<OsString>,
    target: &Path,
    original: &Path,
) -> LocalResult<()> {
    for component in target.components() {
        match component {
            Component::Normal(name) => resolved.push(name.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved.pop().is_none() {
                    return Err(symlink_error(
                        original,
                        LocalFileErrorKind::InvalidPath,
                        "symbolic-link target escaped the authority root",
                    ));
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(symlink_error(
                    original,
                    LocalFileErrorKind::InvalidPath,
                    "absolute symbolic-link targets are outside the authority",
                ));
            }
        }
    }
    Ok(())
}

/// Builds a validated relative path from normalized native components.
fn relative_from_components<'component>(
    components: impl Iterator<Item = &'component OsString>,
) -> LocalResult<RelativePath> {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    RelativePath::parse(&path)
}

/// Creates a structured symlink-policy or containment failure.
fn symlink_error(
    path: &Path,
    kind: LocalFileErrorKind,
    reason: &'static str,
) -> LocalFileError {
    LocalFileError::new(kind, LocalFileOperation::BindPath)
        .with_path(path.to_path_buf())
        .with_reason(reason)
}
