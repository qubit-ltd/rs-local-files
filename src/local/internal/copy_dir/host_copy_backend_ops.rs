// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive enumeration and symbolic-link dispatch for directory copies.
// Private behavior is covered through public integration tests.

use std::collections::HashSet;
use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::path::Path;

use super::super::directory_identity::DirectoryIdentity;
use super::copy_dir_frame::CopyDirFrame;
use super::copy_dir_result::CopyDirResult;
use super::destination::ensure_copy_destination_dir;
use super::error::copy_dir_error;
use super::error::record_created_directory;
use super::error::record_overwritten_entry;
use super::error::record_skipped_file;
use super::error::with_copy_context;
use super::host_copy_backend::HostCopyBackend;
use super::source::inspect_copy_source_directory;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::LocalCopyFailureState;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;
use crate::local::internal::copy_tree;

/// Copies one source directory tree without recursive function calls.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `scope_root` - Optional canonical boundary for followed directory links.
/// * `stats` - Mutable statistics accumulator.
///
/// # Errors
///
/// Returns a structured error when inspection, traversal, copying, permission
/// preservation, or exact accounting fails.
///
/// # Panics
///
/// Panics if the iterative traversal loses its active frame.
pub(super) fn copy_dir_iterative(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    scope_root: Option<&Path>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let mut budget = CopyBudget::new(options);
    let mut backend = HostCopyBackend {
        options,
        destination_root: destination_root.to_path_buf(),
        scope_root: scope_root.map(Path::to_path_buf),
        active_sources: HashSet::new(),
    };
    let Some(root_frame) = enter_copy_directory(
        src,
        dst,
        options,
        destination_root,
        &mut backend.active_sources,
        stats,
        &mut budget,
        0,
    )?
    else {
        return Ok(());
    };
    copy_tree(&mut backend, root_frame, stats, &mut budget)
}

/// Enters one source directory and constructs its traversal frame.
///
/// # Parameters
///
/// * `src` - Source directory.
/// * `dst` - Destination directory.
/// * `options` - Recursive-copy behavior options.
/// * `destination_root` - Canonical destination used for containment checks.
/// * `active_sources` - Filesystem-object ancestor identities used for cycle
///   detection.
/// * `stats` - Mutable statistics accumulator.
/// * `budget` - Shared deadline, depth, and open-directory budget.
/// * `depth` - Source directory depth, with the copied root at zero.
///
/// # Returns
///
/// `Some` owns a lazy reader and permit for the entered directory. `None` means
/// conflict policy skipped the destination without entering its source reader.
///
/// # Errors
///
/// Returns a structured error when inspection, cycle validation, destination
/// preparation, statistics accounting, or directory enumeration fails.
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_copy_directory(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut HashSet<DirectoryIdentity>,
    stats: &mut LocalCopyDirStats,
    budget: &mut CopyBudget,
    depth: usize,
) -> CopyDirResult<Option<CopyDirFrame>> {
    budget
        .check_deadline()
        .map_err(|source| copy_dir_error(LocalCopyDirStage::InspectSource, src, dst, stats, source))?;
    budget
        .check_depth(depth)
        .map_err(|source| copy_dir_error(LocalCopyDirStage::InspectSource, src, dst, stats, source))?;
    let (source_metadata, source_identity) = with_copy_context(
        inspect_copy_source_directory(src, options.symlink_policy(), destination_root),
        LocalCopyDirStage::InspectSource,
        src,
        dst,
        stats,
    )?;
    #[cfg(test)]
    if depth > 0 {
        crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
            crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::HostCopyChildDirectorySourceInspected,
        );
    }
    if active_sources.contains(&source_identity) {
        return Err(copy_dir_error(
            LocalCopyDirStage::InspectSource,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::InvalidInput,
                format!("source directory cycle detected: {}", src.display()),
            ),
        ));
    }
    let mut destination_removed = false;
    let (action, created) = ensure_copy_destination_dir(
        dst,
        options.conflict_policy(),
        options.type_conflict_policy(),
        &mut destination_removed,
        budget,
    )
    .map_err(|source| {
        let error = copy_dir_error(LocalCopyDirStage::PrepareDestination, src, dst, stats, source);
        if destination_removed {
            error.with_failure_state(LocalCopyFailureState::PartiallyPublished)
        } else {
            error
        }
    })?;
    if action == CopyDestinationAction::Skip {
        with_copy_context(
            record_skipped_file(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
        return Ok(None);
    }
    if created {
        with_copy_context(
            record_created_directory(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    if action == CopyDestinationAction::Replace
        || (action == CopyDestinationAction::Merge && options.conflict_policy() == LocalCopyConflictPolicy::Overwrite)
    {
        with_copy_context(
            record_overwritten_entry(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    let directory_permit = budget
        .acquire_directory()
        .map_err(|source| copy_dir_error(LocalCopyDirStage::ReadSourceDirectory, src, dst, stats, source))?;
    let entries = with_copy_context(
        fs::read_dir(src),
        LocalCopyDirStage::ReadSourceDirectory,
        src,
        dst,
        stats,
    )?;
    let _ = active_sources.insert(source_identity.clone());
    Ok(Some(CopyDirFrame::new(
        src.to_path_buf(),
        dst.to_path_buf(),
        depth,
        source_identity,
        source_metadata.permissions(),
        entries,
        directory_permit,
    )))
}

/// Determines whether an allowed symbolic link targets a directory.
///
/// # Parameters
///
/// * `src` - Source symbolic link.
/// * `dst` - Destination path.
/// * `stats` - Read-only snapshot attached to a failed inspection.
/// * `scope_root` - Optional canonical boundary for the resolved link target.
///
/// # Returns
///
/// `true` for a directory target and `false` for a regular-file or missing
/// target when scope canonicalization is not required.
///
/// # Errors
///
/// Returns a structured error when the target cannot be inspected or has an
/// unsupported type.
pub(super) fn symlink_target_is_directory(
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    scope_root: Option<&Path>,
) -> CopyDirResult<bool> {
    if let Some(scope_root) = scope_root {
        let target = with_copy_context(
            fs::canonicalize(src),
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
        )?;
        if !target.starts_with(scope_root) {
            return Err(copy_dir_error(
                LocalCopyDirStage::InspectSourceEntry,
                src,
                dst,
                stats,
                Error::new(
                    ErrorKind::InvalidInput,
                    format!("followed symbolic-link directory escaped copy scope: {}", src.display()),
                ),
            ));
        }
    }
    let target_metadata = match with_copy_context(
        fs::metadata(src),
        LocalCopyDirStage::InspectSourceEntry,
        src,
        dst,
        stats,
    ) {
        Ok(metadata) => metadata,
        Err(error) if error.error().kind() == ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    };
    if target_metadata.is_dir() {
        Ok(true)
    } else if target_metadata.is_file() {
        Ok(false)
    } else {
        Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!("unsupported symbolic link target type: {}", src.display(),),
            ),
        ))
    }
}

#[cfg(all(test, unix))]
mod error_contract_tests {
    use std::collections::HashSet;
    use std::fs;
    use std::io::ErrorKind;
    use std::os::unix::net::UnixListener;

    use super::HostCopyBackend;
    use super::enter_copy_directory;
    use super::symlink_target_is_directory;
    use crate::LocalCopyDirOptions;
    use crate::LocalCopyDirStage;
    use crate::LocalCopyDirStats;
    use crate::local::CopyBudget;
    use crate::local::CopyMetadataEffect;
    use crate::local::internal::CopyTreeBackend;

    #[test]
    fn test_entering_a_directory_rejects_expired_deadlines_and_depth_before_mutation() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let destination = fixture.path().join("destination");
        fs::create_dir(&source).expect("source directory");
        fs::write(source.join("payload"), b"keep").expect("source payload");
        for (options, depth, kind) in [
            (
                LocalCopyDirOptions::new().with_deadline(std::time::Duration::ZERO),
                0,
                ErrorKind::TimedOut,
            ),
            (
                LocalCopyDirOptions::new().with_max_depth(0),
                1,
                ErrorKind::QuotaExceeded,
            ),
        ] {
            let mut stats = LocalCopyDirStats::default();
            let mut budget = CopyBudget::new(options);
            let mut active_sources = HashSet::new();
            let error = enter_copy_directory(
                &source,
                &destination,
                options,
                &destination,
                &mut active_sources,
                &mut stats,
                &mut budget,
                depth,
            )
            .err()
            .expect("directory entry must obey the already active budget");
            assert_eq!(error.stage(), LocalCopyDirStage::InspectSource);
            assert_eq!(error.source_path(), source);
            assert_eq!(error.destination_path(), destination);
            assert_eq!(error.error().kind(), kind);
            assert_eq!(stats, LocalCopyDirStats::default());
            assert!(active_sources.is_empty());
            assert!(!destination.exists());
            assert_eq!(fs::read(source.join("payload")).expect("source should remain"), b"keep");
        }
    }

    #[test]
    fn test_finishing_a_removed_destination_reports_permission_stage_and_preserves_coordinates() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let destination = fixture.path().join("destination");
        fs::create_dir(&source).expect("source");
        let options = LocalCopyDirOptions::new().preserve_permissions();
        let mut stats = LocalCopyDirStats::default();
        let mut budget = CopyBudget::new(options);
        let mut backend = HostCopyBackend {
            options,
            destination_root: destination.clone(),
            scope_root: None,
            active_sources: HashSet::new(),
        };
        let frame = enter_copy_directory(
            &source,
            &destination,
            options,
            &destination,
            &mut backend.active_sources,
            &mut stats,
            &mut budget,
            0,
        )
        .expect("real frame")
        .expect("source entered");
        let recorded = stats;
        fs::remove_dir(&destination).expect("concurrent destination removal");
        let error = backend
            .finish_frame(frame, &mut stats)
            .expect_err("permission restoration must fail");
        assert_eq!(error.stage(), LocalCopyDirStage::PreservePermissions);
        assert_eq!(error.source_path(), source);
        assert_eq!(error.destination_path(), destination);
        assert_eq!(error.error().kind(), ErrorKind::NotFound);
        let mut expected = recorded;
        expected.metadata_effect = CopyMetadataEffect::Indeterminate;
        assert_eq!(error.stats(), &expected);
        assert!(backend.active_sources.is_empty());
        assert!(source.is_dir());
    }

    #[test]
    fn test_followed_link_checks_scope_target_kind_and_dangling_target() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("link");
        let destination = fixture.path().join("destination");
        let boundary = fixture.path().join("boundary");
        fs::create_dir(&boundary).expect("boundary");
        let socket = fixture.path().join("socket");
        let _listener = UnixListener::bind(&socket).expect("socket");
        std::os::unix::fs::symlink(&socket, &source).expect("source link");
        let stats = LocalCopyDirStats::default();
        let error = symlink_target_is_directory(&source, &destination, &stats, None)
            .expect_err("socket is not a copyable target");
        assert_eq!(error.stage(), LocalCopyDirStage::InspectSourceEntry);
        assert_eq!(error.error().kind(), ErrorKind::Unsupported);
        let canonical_boundary = fs::canonicalize(&boundary).expect("canonical boundary");
        let error = symlink_target_is_directory(&source, &destination, &stats, Some(&canonical_boundary))
            .expect_err("link escapes boundary");
        assert_eq!(error.error().kind(), ErrorKind::InvalidInput);
        assert_eq!(error.stats(), &stats);
        fs::remove_file(&source).expect("replace link fixture");
        std::os::unix::fs::symlink("missing", &source).expect("dangling source");
        assert!(!symlink_target_is_directory(&source, &destination, &stats, None).expect("dangling leaf"));
        let error = symlink_target_is_directory(&source, &destination, &stats, Some(&canonical_boundary))
            .expect_err("scoped dangling target cannot canonicalize");
        assert_eq!(error.error().kind(), ErrorKind::NotFound);
    }
}
