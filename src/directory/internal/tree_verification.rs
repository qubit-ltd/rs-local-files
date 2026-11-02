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
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalObservationRequirement;
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
        .map(|duration| {
            started
                .checked_add(duration)
                .ok_or_else(|| resource_error(stage, Path::new("."), "tree validation deadline overflowed", None))
        })
        .transpose()?;
    check_deadline(stage, deadline)?;

    let root = stage
        .directory()
        .observe()
        .map_err(|error| context_error(error, stage, Path::new("."), "tree root observation failed"))?;
    if root.identity() != stage.identity() {
        return Err(
            LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                .with_path(stage.directory().diagnostic_path().to_path_buf())
                .with_reason("staged tree root identity changed"),
        );
    }
    verify_root_privacy(stage, options)?;
    check_deadline(stage, deadline)?;

    if expected.is_none() && !requires_tree_walk(options) {
        return Ok(());
    }

    let limits = options.limits();
    let expected_name_bytes = if let Some(entries) = expected {
        if let Some(maximum) = limits.max_entries()
            && entries.len() > maximum
        {
            return Err(resource_error(
                stage,
                Path::new("."),
                "tree validation entry limit exceeded",
                Some(
                    LocalResourceLimitError::new(LocalResourceKind::Entry, maximum, maximum, entries.len())
                        .with_observed(entries.len()),
                ),
            ));
        }
        let mut bytes = 0_usize;
        for entry in entries {
            check_deadline(stage, deadline)?;
            let requested = entry.relative_path().as_os_str().as_encoded_bytes().len();
            let next = bytes.checked_add(requested).ok_or_else(|| {
                resource_overflow(
                    stage,
                    entry.relative_path(),
                    LocalResourceKind::SeenNameBytes,
                    "tree validation name-byte count overflowed",
                )
            })?;
            if let Some(maximum) = limits.max_seen_name_bytes()
                && next > maximum
            {
                return Err(resource_error(
                    stage,
                    entry.relative_path(),
                    "tree validation name-byte limit exceeded",
                    Some(
                        LocalResourceLimitError::new(
                            LocalResourceKind::SeenNameBytes,
                            maximum,
                            maximum.saturating_sub(bytes),
                            requested,
                        )
                        .with_observed(next),
                    ),
                ));
            }
            bytes = next;
        }
        bytes
    } else {
        0
    };
    let mut expected_index = if let Some(entries) = expected {
        let mut index = ExpectedIndex::new();
        for entry in entries {
            check_deadline(stage, deadline)?;
            index.try_reserve(1).map_err(|_| {
                resource_error(
                    stage,
                    entry.relative_path(),
                    "tree validation expected-set allocation failed",
                    None,
                )
            })?;
            if index.insert(entry.relative_path(), (entry.kind(), false)).is_some() {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::ValidateName)
                        .with_path(stage.directory().diagnostic_path().join(entry.relative_path()))
                        .with_reason("duplicate expected tree entry path"),
                );
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
    let observer = stage
        .observe_tree_with_options(&walk_limits)
        .map_err(|error| context_error(error, stage, Path::new("."), "tree traversal could not start"))?;
    for result in observer {
        check_deadline(stage, deadline)?;
        let item =
            result.map_err(|error| context_error(error, stage, Path::new("."), "tree entry observation failed"))?;
        let path = item.relative_path();
        let kind = item.observation().metadata().kind();
        let depth = path.components().count();
        if limits.max_depth().is_some_and(|maximum| depth > maximum) {
            return Err(tree_limit(
                stage,
                path,
                LocalResourceLimitError::new(LocalResourceKind::Depth, limits.max_depth().unwrap_or(0), 0, depth)
                    .with_observed(depth),
                "tree validation depth limit exceeded",
            ));
        }
        count = count.checked_add(1).ok_or_else(|| {
            resource_overflow(
                stage,
                path,
                LocalResourceKind::Entry,
                "tree validation entry count overflowed",
            )
        })?;
        if limits.max_entries().is_some_and(|maximum| count > maximum) {
            return Err(tree_limit(
                stage,
                path,
                LocalResourceLimitError::new(
                    LocalResourceKind::Entry,
                    limits.max_entries().unwrap_or(0),
                    limits
                        .max_entries()
                        .unwrap_or(0)
                        .saturating_sub(count.saturating_sub(1)),
                    1,
                )
                .with_observed(count),
                "tree validation entry limit exceeded",
            ));
        }
        let requested = path.as_os_str().as_encoded_bytes().len();
        let next_name_bytes = name_bytes.checked_add(requested).ok_or_else(|| {
            resource_overflow(
                stage,
                path,
                LocalResourceKind::SeenNameBytes,
                "tree validation name-byte count overflowed",
            )
        })?;
        if let Some(maximum) = limits.max_seen_name_bytes()
            && next_name_bytes > maximum
        {
            return Err(tree_limit(
                stage,
                path,
                LocalResourceLimitError::new(
                    LocalResourceKind::SeenNameBytes,
                    maximum,
                    maximum.saturating_sub(name_bytes),
                    requested,
                )
                .with_observed(next_name_bytes),
                "tree validation name-byte limit exceeded",
            ));
        }
        name_bytes = next_name_bytes;

        if (options.rejects_symlinks() && kind == LocalFileKind::Symlink)
            || (options.rejects_special_files()
                && !matches!(
                    kind,
                    LocalFileKind::File | LocalFileKind::Directory | LocalFileKind::Symlink
                ))
        {
            let reason = if kind == LocalFileKind::Symlink {
                "symbolic link rejected by tree validation"
            } else {
                "special file rejected by tree validation"
            };
            return Err(requirement_error(stage, path, reason));
        }

        if let Some(index) = expected_index.as_mut() {
            let Some((expected_kind, was_seen)) = index.get_mut(path) else {
                return Err(requirement_error(stage, path, "unexpected tree entry"));
            };
            if *expected_kind != kind {
                return Err(requirement_error(stage, path, "tree entry kind mismatch")
                    .with_detail(format!("expected {expected_kind:?}, observed {kind:?}")));
            }
            if *was_seen {
                return Err(requirement_error(stage, path, "tree entry was observed more than once"));
            }
            *was_seen = true;
        }

        verify_entry_privacy(stage, path, kind, item.observation().identity(), options)?;
        if options.rejects_external_hardlinks() && kind == LocalFileKind::File {
            let (current, native_count) = stage
                .directory()
                .observe_entry_with_link_count(path)
                .map_err(|error| context_error(error, stage, path, "tree hard-link metadata observation failed"))?;
            if current.identity() != item.observation().identity() {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                        .with_path(stage.directory().diagnostic_path().join(path))
                        .with_reason("tree entry identity changed during hard-link verification"),
                );
            }
            let native_count = native_count.ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                    .with_path(stage.directory().diagnostic_path().join(path))
                    .with_reason("native hard-link count is unavailable")
            })?;
            let identity = current.identity().cloned().ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                    .with_path(stage.directory().diagnostic_path().join(path))
                    .with_reason("native file identity is unavailable for hard-link verification")
            })?;
            if let Some((observed_count, occurrences)) = link_counts.get_mut(&identity) {
                if *observed_count != native_count {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    )
                    .with_path(stage.directory().diagnostic_path().join(path))
                    .with_reason("tree entry link count changed during verification"));
                }
                *occurrences = occurrences.checked_add(1).ok_or_else(|| {
                    resource_overflow(stage, path, LocalResourceKind::Entry, "tree hard-link count overflowed")
                })?;
            } else {
                link_counts
                    .try_reserve(1)
                    .map_err(|_| resource_error(stage, path, "tree hard-link index allocation failed", None))?;
                link_counts.insert(identity, (native_count, 1));
            }
        }
    }
    check_deadline(stage, deadline)?;

    if let Some(index) = expected_index.as_ref() {
        for (expected_path, (_, was_seen)) in index.iter() {
            check_deadline(stage, deadline)?;
            if !was_seen {
                return Err(requirement_error(stage, expected_path, "missing expected tree entry"));
            }
        }
    }
    for (native_count, occurrences) in link_counts.values() {
        check_deadline(stage, deadline)?;
        if u64::try_from(*occurrences).is_ok_and(|seen| native_count > &seen) {
            return Err(requirement_error(
                stage,
                Path::new("."),
                "tree contains external hard links",
            ));
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
            .root_authority()
            .map_err(|error| {
                context_error(
                    error,
                    stage,
                    Path::new("."),
                    "tree root privacy authority could not be opened",
                )
            })?
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
            .open_child_for_access()
            .map_err(|error| {
                context_error(
                    error,
                    stage,
                    Path::new("."),
                    "tree root privacy authority could not be opened",
                )
            })?;
        let observation = crate::observation::query::query_observation(&root, LocalObservationRequirement::Required)
            .map_err(|error| context_error(error, stage, Path::new("."), "tree root privacy observation failed"))?;
        if observation.identity() != stage.identity() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                    .with_path(stage.directory().diagnostic_path().to_path_buf())
                    .with_reason("staged tree root identity changed during privacy verification"),
            );
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
        return Err(requirement_error(
            stage,
            path,
            "privacy requirement excludes this tree entry kind",
        ));
    }
    let identity = identity.ok_or_else(|| {
        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
            .with_path(stage.directory().diagnostic_path().join(path))
            .with_reason("entry identity is unavailable for privacy verification")
    })?;
    #[cfg(unix)]
    let file = crate::directory::staged::open_unix_tree_entry(stage, path, kind)
        .map_err(|error| context_error(error, stage, path, "tree entry privacy authority could not be opened"))?;
    #[cfg(windows)]
    let file = crate::directory::staged::open_windows_tree_entry(stage, path, kind)
        .map_err(|error| context_error(error, stage, path, "tree entry privacy authority could not be opened"))?;
    let current = crate::observation::query::query_observation(&file, LocalObservationRequirement::Required)
        .map_err(|error| context_error(error, stage, path, "tree entry privacy observation failed"))?;
    if current.identity() != Some(identity) {
        return Err(
            LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                .with_path(stage.directory().diagnostic_path().join(path))
                .with_reason("tree entry identity changed during privacy verification"),
        );
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
        .with_reason("tree entry privacy or owner-access policy verification failed")
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
        .with_reason("tree entry privacy or owner-access policy verification failed")
    })?;
    Ok(())
}

/// Associates a native access error with the operation and diagnostic path.
fn file_error(operation: LocalFileOperation, path: &Path, source: std::io::Error) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, source)
        .with_reason("tree privacy or owner-access policy verification failed")
}

fn context_error(
    error: LocalFileError,
    stage: &LocalStagedDirectory,
    relative: &Path,
    reason: &'static str,
) -> LocalFileError {
    if error.path().is_some() {
        error.with_reason(reason)
    } else {
        error
            .with_path(stage.directory().diagnostic_path().join(relative))
            .with_reason(reason)
    }
}

/// Returns a resource-limit error when a cooperative deadline has elapsed.
fn check_deadline(stage: &LocalStagedDirectory, deadline: Option<Instant>) -> LocalResult<()> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        Err(resource_error(
            stage,
            Path::new("."),
            "tree validation deadline exceeded",
            None,
        ))
    } else {
        Ok(())
    }
}

/// Enforces the logical encoded-name byte budget, excluding allocator overhead.
/// Creates the common error used when complete traversal exceeds a budget.
fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

/// Creates the error used when an observed tree violates a requested condition.
fn requirement_error(stage: &LocalStagedDirectory, relative: &Path, reason: &'static str) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
        .with_path(stage.directory().diagnostic_path().join(relative))
        .with_reason(reason)
}

fn resource_error(
    stage: &LocalStagedDirectory,
    relative: &Path,
    reason: &'static str,
    facts: Option<LocalResourceLimitError>,
) -> LocalFileError {
    let path = stage.directory().diagnostic_path().join(relative);
    match facts {
        Some(facts) => {
            LocalFileError::from_resource_limit(LocalFileOperation::Read, Some(path), facts).with_reason(reason)
        }
        None => resource_limit().with_path(path).with_reason(reason),
    }
}

fn tree_limit(
    stage: &LocalStagedDirectory,
    relative: &Path,
    facts: LocalResourceLimitError,
    reason: &'static str,
) -> LocalFileError {
    resource_error(stage, relative, reason, Some(facts))
}

fn resource_overflow(
    stage: &LocalStagedDirectory,
    relative: &Path,
    resource: LocalResourceKind,
    reason: &'static str,
) -> LocalFileError {
    resource_error(
        stage,
        relative,
        reason,
        Some(LocalResourceLimitError::new(resource, usize::MAX, 0, 1).with_overflow()),
    )
}

#[cfg(test)]
mod missing_path_tests {
    use std::path::Path;

    use super::resource_overflow;
    use super::verify_entry_privacy;
    use crate::LocalFileKind;
    use crate::LocalFileSystem;
    use crate::LocalResourceKind;
    use crate::directory::LocalTreeValidationOptions;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::policy::LocalPrivacyRequirement;

    #[test]
    fn missing_entry_identity_is_reported_before_privacy_reopen() {
        let fixture = tempfile::tempdir().expect("fixture");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let parent = filesystem
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent directory");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("staged directory");

        let error = verify_entry_privacy(
            &stage,
            Path::new("payload"),
            LocalFileKind::File,
            None,
            &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect_err("required privacy verification needs an observed identity");
        assert_eq!(error.kind(), crate::LocalFileErrorKind::Unsupported);
        assert_eq!(
            error.path(),
            Some(stage.directory().diagnostic_path().join("payload").as_path())
        );
        stage.cleanup().expect("cleanup stage");
    }

    #[test]
    fn explicit_resource_overflow_retains_limit_kind_and_path() {
        let fixture = tempfile::tempdir().expect("fixture");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let parent = filesystem
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent directory");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("staged directory");

        let error = resource_overflow(
            &stage,
            Path::new("payload"),
            LocalResourceKind::Entry,
            "entry accounting overflowed",
        );
        assert_eq!(error.kind(), crate::LocalFileErrorKind::ResourceLimit);
        assert_eq!(
            error.path(),
            Some(stage.directory().diagnostic_path().join("payload").as_path())
        );
        stage.cleanup().expect("cleanup stage");
    }

    #[test]
    fn privacy_reopen_failure_keeps_the_entry_path_context() {
        let fixture = tempfile::tempdir().expect("fixture");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let parent = filesystem
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent directory");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("staged directory");
        stage
            .write_file_new(Path::new("payload"), &mut &b"payload"[..], 7)
            .expect("write staged file");
        let identity = stage
            .directory()
            .observe_entry(Path::new("payload"))
            .expect("observe staged file")
            .identity()
            .cloned()
            .expect("entry identity");
        std::fs::remove_file(stage.directory().diagnostic_path().join("payload")).expect("remove staged file");

        let error = verify_entry_privacy(
            &stage,
            Path::new("payload"),
            LocalFileKind::File,
            Some(&identity),
            &LocalTreeValidationOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect_err("privacy handle cannot be reopened after removal");
        assert!(error.path().is_some());
        stage.cleanup().expect("cleanup stage");
    }
}
