// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::directory::LocalStagedDirectory;
use crate::directory::tree_validation::LocalTreeEntry;
use crate::directory::tree_validation::LocalTreeValidationOptions;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalObservationRequirement;
use crate::options::LocalListOptions;
use crate::policy::LocalPrivacyRequirement;
use crate::policy::LocalSymlinkPolicy;

type ExpectedIndex<'a> = HashMap<&'a Path, (LocalFileKind, bool)>;
type LinkCounts = HashMap<LocalFileIdentity, (u64, usize)>;

/// Verifies a staged tree in one entry-only pass.
///
/// Expected paths are indexed by borrowed path references after their encoded
/// bytes are charged. The name-byte ceiling covers these paths plus observed
/// logical paths, not allocator or hash-table overhead. A finite depth is an
/// observation ceiling: actual entries beyond it fail rather than being
/// silently omitted. This checks a sequence of observations and does not
/// provide an atomic filesystem snapshot.
///
/// # Errors
///
/// Returns `ChangedDuringRead` if the retained root or an entry changes,
/// `RequirementNotMet` when the tree violates a requested constraint, and
/// `ResourceLimit` when traversal exceeds a configured budget or deadline.
pub(in crate::directory) fn verify_tree(
    stage: &LocalStagedDirectory,
    expected: Option<&[LocalTreeEntry]>,
    options: &LocalTreeValidationOptions,
) -> LocalResult<()> {
    let started = Instant::now();
    let deadline = options
        .limits()
        .deadline()
        .map(|duration| started.checked_add(duration).ok_or_else(resource_limit))
        .transpose()?;
    check_deadline(deadline)?;

    let root = stage.directory().observe()?;
    if root.identity() != stage.identity() {
        return Err(LocalFileError::new(
            LocalFileErrorKind::ChangedDuringRead,
            LocalFileOperation::Metadata,
        ));
    }
    verify_root_privacy(stage, options)?;
    check_deadline(deadline)?;

    if expected.is_none() && !requires_tree_walk(options) {
        return Ok(());
    }

    let limits = options.limits();
    let expected_name_bytes = if let Some(entries) = expected {
        if limits.max_entries().is_some_and(|maximum| entries.len() > maximum) {
            return Err(resource_limit());
        }
        let mut bytes = 0_usize;
        for entry in entries {
            check_deadline(deadline)?;
            bytes = bytes
                .checked_add(entry.relative_path().as_os_str().as_encoded_bytes().len())
                .ok_or_else(resource_limit)?;
        }
        check_name_budget(limits, bytes)?;
        bytes
    } else {
        0
    };
    let mut expected_index = if let Some(entries) = expected {
        let mut index = ExpectedIndex::new();
        for entry in entries {
            check_deadline(deadline)?;
            index.try_reserve(1).map_err(|_| resource_limit())?;
            if index.insert(entry.relative_path(), (entry.kind(), false)).is_some() {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidOptions,
                    LocalFileOperation::ValidateName,
                ));
            }
        }
        Some(index)
    } else {
        None
    };

    let mut name_bytes = expected_name_bytes;
    let mut count = 0_usize;
    let mut link_counts = LinkCounts::new();
    let walk_limits = limits
        .without_max_depth()
        .with_recursive()
        .with_symlink_policy(LocalSymlinkPolicy::Reject)
        .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
    let observer = stage.observe_tree_with_options(&walk_limits)?;
    for result in observer {
        check_deadline(deadline)?;
        let item = result?;
        let path = item.relative_path();
        let kind = item.observation().metadata().kind();
        let depth = path.components().count();
        if limits.max_depth().is_some_and(|maximum| depth > maximum) {
            return Err(resource_limit());
        }
        count = count.checked_add(1).ok_or_else(resource_limit)?;
        if limits.max_entries().is_some_and(|maximum| count > maximum) {
            return Err(resource_limit());
        }
        name_bytes = name_bytes
            .checked_add(path.as_os_str().as_encoded_bytes().len())
            .ok_or_else(resource_limit)?;
        check_name_budget(limits, name_bytes)?;

        if (options.rejects_symlinks() && kind == LocalFileKind::Symlink)
            || (options.rejects_special_files()
                && !matches!(
                    kind,
                    LocalFileKind::File | LocalFileKind::Directory | LocalFileKind::Symlink
                ))
        {
            return Err(requirement_not_met());
        }

        if let Some(index) = expected_index.as_mut() {
            let Some((expected_kind, was_seen)) = index.get_mut(path) else {
                return Err(requirement_not_met());
            };
            if *expected_kind != kind || *was_seen {
                return Err(requirement_not_met());
            }
            *was_seen = true;
        }

        verify_entry_privacy(stage, path, kind, item.observation().identity(), options)?;
        if options.rejects_external_hardlinks() && kind == LocalFileKind::File {
            let (current, native_count) = stage.directory().observe_entry_with_link_count(path)?;
            if current.identity() != item.observation().identity() {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::ChangedDuringRead,
                    LocalFileOperation::Metadata,
                ));
            }
            let native_count = native_count.ok_or_else(unsupported)?;
            let identity = current.identity().cloned().ok_or_else(unsupported)?;
            if let Some((observed_count, occurrences)) = link_counts.get_mut(&identity) {
                if *observed_count != native_count {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    ));
                }
                *occurrences = occurrences.checked_add(1).ok_or_else(resource_limit)?;
            } else {
                link_counts.try_reserve(1).map_err(|_| resource_limit())?;
                link_counts.insert(identity, (native_count, 1));
            }
        }
    }
    check_deadline(deadline)?;

    if let Some(index) = expected_index.as_ref() {
        for (_, was_seen) in index.values() {
            check_deadline(deadline)?;
            if !was_seen {
                return Err(requirement_not_met());
            }
        }
    }
    for (native_count, occurrences) in link_counts.values() {
        check_deadline(deadline)?;
        if u64::try_from(*occurrences).is_ok_and(|seen| native_count > &seen) {
            return Err(requirement_not_met());
        }
    }
    Ok(())
}

/// Determines whether any configured constraint requires enumerating entries.
fn requires_tree_walk(options: &LocalTreeValidationOptions) -> bool {
    let limits = options.limits();
    options.exact_entries().is_some()
        || options.rejects_symlinks()
        || options.rejects_special_files()
        || options.rejects_external_hardlinks()
        || options.privacy() == LocalPrivacyRequirement::Required
        || options.owner_access_policy().is_some()
        || limits.max_open_directories().is_some()
        || limits.max_depth().is_some()
        || limits.max_entries().is_some()
        || limits.max_seen_name_bytes().is_some()
        || limits.deadline().is_some()
}

/// Checks privacy of the retained stage root when privacy is required.
fn verify_root_privacy(stage: &LocalStagedDirectory, options: &LocalTreeValidationOptions) -> LocalResult<()> {
    if options.privacy() != LocalPrivacyRequirement::Required && options.owner_access_policy().is_none() {
        return Ok(());
    }
    #[cfg(unix)]
    {
        let file = stage
            .directory()
            .root_authority()?
            .try_clone_authority()
            .map_err(|source| {
                file_error(
                    LocalFileOperation::Metadata,
                    stage.directory().diagnostic_path(),
                    source,
                )
            })?;
        let result = if let Some(policy) = options.owner_access_policy() {
            crate::temp::internal::privacy_unix::verify_owner_access(&file, true, policy)
        } else {
            crate::temp::internal::privacy_unix::verify_current_user_access(&file, true, false)
        };
        result.map_err(|source| {
            file_error(
                LocalFileOperation::Metadata,
                stage.directory().diagnostic_path(),
                source,
            )
        })?;
    }
    #[cfg(windows)]
    {
        let root = stage
            .owner
            .as_ref()
            .expect("stage owner exists")
            .open_child_for_access()?;
        let observation = crate::observation::query::query_observation(&root, LocalObservationRequirement::Required)?;
        if observation.identity() != stage.identity() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ChangedDuringRead,
                LocalFileOperation::Metadata,
            ));
        }
        let result = if let Some(policy) = options.owner_access_policy() {
            crate::temp::internal::privacy_windows::verify_owner_access(&root, true, policy)
        } else {
            crate::temp::internal::privacy_windows::verify_current_user_access(&root, true, false)
        };
        result.map_err(|source| {
            file_error(
                LocalFileOperation::Metadata,
                stage.directory().diagnostic_path(),
                source,
            )
        })?;
    }
    Ok(())
}

/// Checks privacy for an observed ordinary file or directory entry.
fn verify_entry_privacy(
    stage: &LocalStagedDirectory,
    path: &Path,
    kind: LocalFileKind,
    identity: Option<&LocalFileIdentity>,
    options: &LocalTreeValidationOptions,
) -> LocalResult<()> {
    if options.privacy() != LocalPrivacyRequirement::Required && options.owner_access_policy().is_none() {
        return Ok(());
    }
    if !matches!(kind, LocalFileKind::File | LocalFileKind::Directory) {
        return Err(requirement_not_met());
    }
    let identity = identity.ok_or_else(unsupported)?;
    #[cfg(unix)]
    let file = crate::directory::staged::open_unix_tree_entry(stage, path, kind)?;
    #[cfg(windows)]
    let file = crate::directory::staged::open_windows_tree_entry(stage, path, kind)?;
    let current = crate::observation::query::query_observation(&file, LocalObservationRequirement::Required)?;
    if current.identity() != Some(identity) {
        return Err(LocalFileError::new(
            LocalFileErrorKind::ChangedDuringRead,
            LocalFileOperation::Metadata,
        ));
    }
    #[cfg(unix)]
    (if let Some(policy) = options.owner_access_policy() {
        crate::temp::internal::privacy_unix::verify_owner_access(&file, kind == LocalFileKind::Directory, policy)
    } else {
        crate::temp::internal::privacy_unix::verify_current_user_access(&file, kind == LocalFileKind::Directory, false)
    })
    .map_err(|source| {
        file_error(
            LocalFileOperation::Metadata,
            &stage.directory().diagnostic_path().join(path),
            source,
        )
    })?;
    #[cfg(windows)]
    (if let Some(policy) = options.owner_access_policy() {
        crate::temp::internal::privacy_windows::verify_owner_access(&file, kind == LocalFileKind::Directory, policy)
    } else {
        crate::temp::internal::privacy_windows::verify_current_user_access(
            &file,
            kind == LocalFileKind::Directory,
            false,
        )
    })
    .map_err(|source| {
        file_error(
            LocalFileOperation::Metadata,
            &stage.directory().diagnostic_path().join(path),
            source,
        )
    })?;
    Ok(())
}

/// Associates a native access error with the operation and diagnostic path.
fn file_error(operation: LocalFileOperation, path: &Path, source: std::io::Error) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, source)
}

/// Returns a resource-limit error when a cooperative deadline has elapsed.
fn check_deadline(deadline: Option<Instant>) -> LocalResult<()> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        Err(resource_limit())
    } else {
        Ok(())
    }
}

/// Enforces the logical encoded-name byte budget, excluding allocator overhead.
fn check_name_budget(options: &LocalListOptions, bytes: usize) -> LocalResult<()> {
    if options.max_seen_name_bytes().is_some_and(|maximum| bytes > maximum) {
        Err(resource_limit())
    } else {
        Ok(())
    }
}

/// Creates the common error used when complete traversal exceeds a budget.
fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

/// Creates the error used when an observed tree violates a requested condition.
fn requirement_not_met() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
}

/// Creates the error used when required native identity or count data is
/// absent.
fn unsupported() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
}
