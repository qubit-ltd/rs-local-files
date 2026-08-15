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
use super::fs;
use super::io;
use super::rooted_io_error;
use super::rooted_path;

/// Validates that the requested rooted listing start exists as a directory.
pub(crate) fn validate_rooted_list_start(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<()> {
    if path.as_os_str().is_empty() {
        let metadata = root.metadata().map_err(|error| {
            rooted_io_error(LocalFileOperation::List, path, error)
        })?;
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::List,
            )
            .with_path(path.to_path_buf()));
        }
        return Ok(());
    }
    let path = resolve_rooted_path(
        root,
        path,
        symlink_policy,
        true,
        LocalFileOperation::List,
    )?;
    let metadata = root.symlink_metadata(&path).map_err(|error| {
        rooted_io_error(LocalFileOperation::List, path.as_path(), error)
    })?;
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(LocalFileError::new(
            LocalFileErrorKind::TypeConflict,
            LocalFileOperation::List,
        )
        .with_path(path.as_path().to_path_buf()));
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
    let relative = rooted_path(path, operation)?;
    if !symlink_policy.follows() {
        return Ok(relative);
    }
    let authority_root = root
        .authority_path()
        .map_err(|error| rooted_io_error(operation, path, error))?;
    let diagnostic = authority_root.join(relative.as_path());
    let mut components = diagnostic.components().peekable();
    let mut current = PathBuf::new();
    let mut has_symlink = false;
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        let is_final = components.peek().is_none();
        if is_final && !follow_final {
            break;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                has_symlink = true;
                break;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(rooted_io_error(operation, path, error));
            }
        }
    }
    if !has_symlink {
        return Ok(relative);
    }

    let resolved = if follow_final {
        fs::canonicalize(&diagnostic)
    } else {
        let parent = diagnostic.parent().unwrap_or(&authority_root);
        fs::canonicalize(parent).map(|parent| {
            parent.join(
                diagnostic
                    .file_name()
                    .expect("validated rooted paths have a final component"),
            )
        })
    }
    .map_err(|error| rooted_io_error(operation, path, error))?;
    let canonical_root = fs::canonicalize(&authority_root)
        .map_err(|error| rooted_io_error(operation, path, error))?;
    if resolved.starts_with(&canonical_root) {
        let relative = resolved
            .strip_prefix(&canonical_root)
            .expect("a contained path has a root prefix");
        let relative = crate::local::LocalRelativePath::new(relative)
            .map_err(|error| rooted_io_error(operation, path, error))?;
        return Ok(relative);
    }
    if symlink_policy == LocalSymlinkPolicy::FollowWithinScope {
        return Err(LocalFileError::new(
            LocalFileErrorKind::InvalidPath,
            operation,
        )
        .with_reason("symbolic-link resolution escaped the rooted scope")
        .with_path(path.to_path_buf()));
    }
    Err(
        LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason(
                "FollowAcrossScope is not supported by Rooted filesystems because Rooted authority cannot escape its opened root",
            )
            .with_path(path.to_path_buf()),
    )
}

/// Synchronizes ancestors that may have gained newly created directories.
pub(crate) fn sync_rooted_copy_parent_chain(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
) -> io::Result<()> {
    let mut parent = target.as_path().parent().map(Path::to_path_buf);
    while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
        let path = crate::local::LocalRelativePath::new(&path)
            .expect("parent of a validated rooted path is valid");
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
        fs::create_dir(directory.path().join("nested"))
            .expect("nested directory should be created");
        fs::write(directory.path().join("nested/payload"), b"payload")
            .expect("payload should be written");
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
        validate_rooted_list_start(
            &root,
            Path::new("nested"),
            LocalSymlinkPolicy::Reject,
        )
        .expect("nested directory should be a valid list start");
        sync_rooted_copy_parent_chain(
            &root,
            &LocalRelativePath::new(path)
                .expect("contained path should validate"),
        )
        .expect("contained parent chain should synchronize");
    }
}
