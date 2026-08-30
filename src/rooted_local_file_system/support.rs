// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted support operations.
// qubit-style: allow source-test-pair

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

/// Validates that the requested rooted listing start exists as a directory.
pub(crate) fn validate_rooted_list_start(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<()> {
    if path.as_os_str().is_empty() {
        let metadata = root
            .metadata()
            .map_err(|error| rooted_io_error(LocalFileOperation::List, path, error))?;
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::List)
                    .with_path(path.to_path_buf()),
            );
        }
        return Ok(());
    }
    let path = resolve_rooted_path_allow_root(root, path, symlink_policy, true, LocalFileOperation::List)?;
    let metadata = if path.as_os_str().is_empty() {
        root.metadata()
            .map_err(|error| rooted_io_error(LocalFileOperation::List, path.as_path(), error))?
    } else {
        let relative = crate::local::LocalRelativePath::new(&path)
            .map_err(|error| rooted_io_error(LocalFileOperation::List, &path, error))?;
        root.symlink_metadata(&relative)
            .map_err(|error| rooted_io_error(LocalFileOperation::List, &path, error))?
    };
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(
            LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::List)
                .with_path(path.as_path().to_path_buf()),
        );
    }
    Ok(())
}

/// Resolves rooted path components while preserving final-entry semantics.
pub(crate) fn resolve_rooted_path(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    let resolved = resolve_rooted_path_allow_root(root, path, symlink_policy, follow_final, operation)?;
    crate::local::LocalRelativePath::new(&resolved)
        .map_err(|error| rooted_io_error(operation, path, error).with_kind(LocalFileErrorKind::InvalidPath))
}

/// Resolves a rooted path while allowing the virtual root as the result.
pub(crate) fn resolve_rooted_path_allow_root(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<PathBuf> {
    let relative = rooted_path(path, operation)?;
    resolve_rooted_symlinks(root, relative, symlink_policy, follow_final, operation, path)
}

/// Expands symlinks from the retained root handle without consulting its
/// diagnostic path or imposing a fixed expansion-count budget.
fn resolve_rooted_symlinks(
    root: &crate::rooted::Root,
    path: crate::local::LocalRelativePath,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
    original: &Path,
) -> LocalResult<PathBuf> {
    use std::collections::HashSet;
    if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("FollowAcrossScope is incompatible with a Rooted filesystem")
            .with_path(original.to_path_buf()));
    }
    let mut pending = steps_from_path(path.as_path(), original, operation)?;
    let mut resolved = Vec::<std::ffi::OsString>::new();
    let mut active_symlinks = HashSet::<SymlinkIdentity>::new();
    while let Some(step) = pending.pop_front() {
        match step {
            ResolutionStep::ResetRoot => resolved.clear(),
            ResolutionStep::Parent => {
                if resolved.pop().is_none() {
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("symbolic-link target escaped the Rooted virtual root")
                        .with_path(original.to_path_buf()));
                }
            }
            ResolutionStep::EndSymlink(identity) => {
                active_symlinks.remove(&identity);
            }
            ResolutionStep::Normal(component) => {
                let candidate =
                    relative_from_components(resolved.iter().chain(std::iter::once(&component)), original, operation)?;
                let is_final = pending.iter().all(|step| matches!(step, ResolutionStep::EndSymlink(_)));
                if is_final && !follow_final {
                    resolved.push(component);
                    continue;
                }
                let metadata = match root.symlink_metadata(&candidate) {
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
                let identity = metadata.native_identity().map_or_else(
                    || SymlinkIdentity::NamespacePath(candidate.as_path().to_path_buf()),
                    |(device, file)| SymlinkIdentity::Native(device, file),
                );
                if !active_symlinks.insert(identity.clone()) {
                    return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                        .with_reason("symbolic-link expansion cycle detected")
                        .with_path(original.to_path_buf()));
                }
                let target = root
                    .read_link(&candidate)
                    .map_err(|error| rooted_io_error(operation, original, error))?;
                let mut replacement = steps_from_path(&target, original, operation)?;
                replacement.push_back(ResolutionStep::EndSymlink(identity));
                replacement.extend(pending);
                pending = replacement;
            }
        }
    }
    Ok(path_from_components(resolved.iter()))
}

/// Converts native path syntax into pending virtual-root resolution steps.
fn steps_from_path(
    path: &Path,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<std::collections::VecDeque<ResolutionStep>> {
    use std::collections::VecDeque;
    use std::path::Component;

    let mut steps = VecDeque::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) => {
                return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
                    .with_reason("native prefixes are invalid in Rooted symbolic-link targets")
                    .with_path(original.to_path_buf()));
            }
            Component::RootDir => steps.push_back(ResolutionStep::ResetRoot),
            Component::CurDir => {}
            Component::ParentDir => steps.push_back(ResolutionStep::Parent),
            Component::Normal(name) => steps.push_back(ResolutionStep::Normal(name.to_os_string())),
        }
    }
    Ok(steps)
}

/// Builds a non-empty rooted relative path from normalized native components.
fn relative_from_components<'component>(
    components: impl Iterator<Item = &'component std::ffi::OsString>,
    original: &Path,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    let mut path = PathBuf::new();
    for component in components {
        path.push(component);
    }
    crate::local::LocalRelativePath::new(&path)
        .map_err(|error| rooted_io_error(operation, original, error).with_kind(LocalFileErrorKind::InvalidPath))
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
pub(crate) fn sync_rooted_copy_parent_chain(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
) -> io::Result<()> {
    let mut parent = target.as_path().parent().map(Path::to_path_buf);
    while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
        let path = crate::local::LocalRelativePath::new(&path).expect("parent of a validated rooted path is valid");
        root.sync_parent(&path)?;
        parent = path.as_path().parent().map(Path::to_path_buf);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use tempfile::tempdir;

    use super::resolve_rooted_path;
    use super::sync_rooted_copy_parent_chain;
    use super::validate_rooted_list_start;
    use crate::LocalFileOperation;
    use crate::LocalSymlinkPolicy;
    use crate::local::LocalRelativePath;
    use crate::rooted::Root;

    /// Verifies rooted support resolves, validates, probes, and synchronizes
    /// ordinary descendants using the retained descriptor authority.
    #[test]
    fn test_rooted_support_operates_on_contained_descendants() {
        let directory = tempdir().expect("temporary root should be created");
        fs::create_dir(directory.path().join("nested")).expect("nested directory should be created");
        fs::write(directory.path().join("nested/payload"), b"payload").expect("payload should be written");
        let root = Root::open(directory.path()).expect("root should open");
        let path = Path::new("nested/payload");

        let resolved = resolve_rooted_path(
            &root,
            path,
            LocalSymlinkPolicy::Reject,
            true,
            LocalFileOperation::Metadata,
        )
        .expect("contained path should resolve");
        assert_eq!(path, resolved.as_path());
        validate_rooted_list_start(&root, Path::new("nested"), LocalSymlinkPolicy::Reject)
            .expect("nested directory should be a valid list start");
        sync_rooted_copy_parent_chain(
            &root,
            &LocalRelativePath::new(path).expect("contained path should validate"),
        )
        .expect("contained parent chain should synchronize");
    }
}
