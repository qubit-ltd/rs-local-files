// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public option-value coverage for local filesystem operations.

use std::{
    hint::black_box,
    path::Path,
    time::Duration,
};

use qubit_local_files::{
    LocalAtomicityRequirement,
    LocalCopyConflictPolicy,
    LocalCopyOptions,
    LocalCopySourceMode,
    LocalCopyTypeConflictPolicy,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalDirectoryReopenPolicy,
    LocalDurabilityRequirement,
    LocalListOptions,
    LocalMetadataPreservePolicy,
    LocalPersistOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalSymlinkPolicy,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWalkErrorPolicy,
    LocalWriteMode,
    LocalWriteOptions,
};

#[cfg(coverage)]
use qubit_local_files::{
    LocalAtomicWriteOptions,
    LocalAtomicCommitError,
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalCopyFailure,
    LocalCopyFailureState,
    LocalCopyStats,
    LocalPersistError,
    LocalPersistFailureState,
    LocalPersistStage,
    NativeWriteMode,
    NativeWriteOpenOptions,
    Permissions,
    PathIoError,
    coverage_with_path_context,
    coverage_is_enabled,
    coverage_take,
    coverage_take_on_nth,
    coverage_decide_copy_destination,
    CoverageCopyDestinationAction,
    coverage_absolute_path,
    coverage_add_path_context,
    coverage_canonicalize_existing_prefix,
    coverage_clean_dir_path,
    coverage_ensure_dir_path,
    coverage_ensure_parent_path,
    coverage_ensure_parent_path_with_sync_dirs,
    coverage_remove_any_path,
    RootedEntryKind,
    RootedMetadata,
    coverage_entry_kind_from_mode,
};

/// Verifies directory and deletion builders retain every configured policy.
#[test]
fn test_directory_and_delete_option_builders_retain_policies() {
    let create = black_box(
        LocalCreateDirectoryOptions::new as fn() -> LocalCreateDirectoryOptions,
    );
    let create_recursive = black_box(
        LocalCreateDirectoryOptions::with_recursive
            as fn(LocalCreateDirectoryOptions) -> LocalCreateDirectoryOptions,
    );
    let create_exists_ok = black_box(
        LocalCreateDirectoryOptions::with_exists_ok
            as fn(LocalCreateDirectoryOptions) -> LocalCreateDirectoryOptions,
    );
    let directory = create_exists_ok(create_recursive(create()));
    assert!(black_box(LocalCreateDirectoryOptions::recursive)(
        &directory
    ));
    assert!(black_box(LocalCreateDirectoryOptions::exists_ok)(
        &directory
    ));

    let delete =
        black_box(LocalDeleteOptions::new as fn() -> LocalDeleteOptions);
    let delete_recursive = black_box(
        LocalDeleteOptions::with_recursive
            as fn(LocalDeleteOptions) -> LocalDeleteOptions,
    );
    let delete_missing_ok = black_box(
        LocalDeleteOptions::with_missing_ok
            as fn(LocalDeleteOptions) -> LocalDeleteOptions,
    );
    let deletion = delete_missing_ok(delete_recursive(delete()));
    assert!(black_box(LocalDeleteOptions::recursive)(&deletion));
    assert!(black_box(LocalDeleteOptions::missing_ok)(&deletion));
}

/// Verifies listing and reader builders retain their traversal and retry data.
#[test]
fn test_list_and_read_option_builders_retain_policies() {
    let list = black_box(LocalListOptions::new as fn() -> LocalListOptions);
    let list_recursive = black_box(
        LocalListOptions::with_recursive
            as fn(LocalListOptions) -> LocalListOptions,
    );
    let list_policy = black_box(
        LocalListOptions::with_symlink_policy
            as fn(LocalListOptions, LocalSymlinkPolicy) -> LocalListOptions,
    );
    let list_max_depth = black_box(
        LocalListOptions::with_max_depth
            as fn(LocalListOptions, usize) -> LocalListOptions,
    );
    let list_max_handles = black_box(
        LocalListOptions::with_max_open_directories
            as fn(LocalListOptions, usize) -> LocalListOptions,
    );
    let listing = list_max_handles(
        list_max_depth(
            list_policy(
                list_recursive(list()),
                LocalSymlinkPolicy::FollowWithinScope,
            ),
            3,
        ),
        7,
    );
    assert!(black_box(LocalListOptions::recursive)(&listing));
    assert_eq!(
        Some(LocalSymlinkPolicy::FollowWithinScope),
        black_box(LocalListOptions::symlink_policy)(&listing),
    );
    assert_eq!(black_box(LocalListOptions::max_depth)(&listing), Some(3));
    assert_eq!(
        black_box(LocalListOptions::max_open_directories)(&listing),
        7
    );
    assert_eq!(
        LocalWalkErrorPolicy::FailFast,
        black_box(LocalListOptions::error_policy)(&listing),
    );
    let listing = black_box(
        LocalListOptions::with_reopen_policy
            as fn(
                LocalListOptions,
                LocalDirectoryReopenPolicy,
            ) -> LocalListOptions,
    )(listing, LocalDirectoryReopenPolicy::Fail);
    let listing = black_box(
        LocalListOptions::with_error_policy
            as fn(LocalListOptions, LocalWalkErrorPolicy) -> LocalListOptions,
    )(listing, LocalWalkErrorPolicy::Continue);
    assert_eq!(
        LocalDirectoryReopenPolicy::Fail,
        black_box(LocalListOptions::reopen_policy)(&listing),
    );
    assert_eq!(
        LocalWalkErrorPolicy::Continue,
        black_box(LocalListOptions::error_policy)(&listing),
    );

    let timeout = Duration::from_millis(25);
    let reader = black_box(LocalReadOptions::new as fn() -> LocalReadOptions)();
    let reader = black_box(
        LocalReadOptions::with_open_retry_timeout
            as fn(LocalReadOptions, Duration) -> LocalReadOptions,
    )(reader, timeout);
    assert_eq!(
        black_box(LocalReadOptions::open_retry_timeout)(&reader),
        Some(timeout)
    );
}

/// Verifies copy, rename, and write builders preserve all publication rules.
#[test]
fn test_copy_rename_and_write_option_builders_retain_policies() {
    let copy = black_box(LocalCopyOptions::new())
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
        .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .with_file_source()
        .with_tree_source()
        .with_parent()
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Required);
    assert_eq!(copy.conflict(), LocalCopyConflictPolicy::Overwrite);
    assert_eq!(copy.type_conflict(), LocalCopyTypeConflictPolicy::Replace);
    assert_eq!(
        copy.preserve_metadata(),
        LocalMetadataPreservePolicy::Permissions
    );
    assert_eq!(
        copy.symlink_policy_override(),
        Some(LocalSymlinkPolicy::FollowWithinScope)
    );
    assert_eq!(copy.source_mode(), LocalCopySourceMode::Tree);
    assert!(copy.creates_parent());
    assert_eq!(copy.atomicity(), LocalAtomicityRequirement::Required);
    assert_eq!(copy.durability(), LocalDurabilityRequirement::Required);

    let rename = black_box(LocalRenameOptions::new())
        .with_overwrite()
        .with_durability(LocalDurabilityRequirement::Preferred);
    assert!(rename.overwrite());
    assert_eq!(rename.durability(), LocalDurabilityRequirement::Preferred);

    let timeout = Duration::from_secs(1);
    let writer = black_box(LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .with_parent()
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Preferred)
        .with_open_retry_timeout(timeout);
    assert_eq!(writer.mode(), LocalWriteMode::CreateNew);
    assert!(writer.creates_parent());
    assert_eq!(writer.atomicity(), LocalAtomicityRequirement::Required);
    assert_eq!(writer.durability(), LocalDurabilityRequirement::Preferred);
    assert_eq!(writer.open_retry_timeout(), Some(timeout));
}

/// Verifies temporary-resource builders retain path and naming configuration.
#[test]
fn test_temporary_resource_option_builders_retain_configuration() {
    let parent = Path::new("temporary-parent");
    let with_suffix = black_box(
        LocalTempFileOptions::with_suffix
            as fn(LocalTempFileOptions, &str) -> LocalTempFileOptions,
    );
    let file = with_suffix(
        black_box(LocalTempFileOptions::new())
            .with_parent(parent)
            .with_prefix("file-"),
        ".tmp",
    )
    .with_max_attempts(4)
    .with_create_parent();
    assert_eq!(file.parent(), Some(parent));
    assert_eq!(file.prefix(), Some("file-"));
    assert_eq!(file.suffix(), Some(".tmp"));
    assert_eq!(file.max_attempts(), 4);
    assert!(file.creates_parent());

    let directory = black_box(LocalTempDirectoryOptions::new())
        .with_parent(parent)
        .with_prefix("directory-")
        .with_suffix(".tmp")
        .with_max_attempts(5);
    assert_eq!(directory.parent(), Some(parent));
    assert_eq!(directory.prefix(), Some("directory-"));
    assert_eq!(directory.suffix(), Some(".tmp"));
    assert_eq!(directory.max_attempts(), 5);
    assert!(!directory.creates_parent());
    assert!(directory.with_create_parent().creates_parent());
}

/// Verifies every option type exposes the documented conservative default.
#[test]
fn test_option_defaults_match_their_constructors() {
    let create_default = black_box(
        LocalCreateDirectoryOptions::default
            as fn() -> LocalCreateDirectoryOptions,
    );
    let delete_default =
        black_box(LocalDeleteOptions::default as fn() -> LocalDeleteOptions);
    let list_default =
        black_box(LocalListOptions::default as fn() -> LocalListOptions);
    let read_default =
        black_box(LocalReadOptions::default as fn() -> LocalReadOptions);
    let copy_default =
        black_box(LocalCopyOptions::default as fn() -> LocalCopyOptions);
    let rename_default =
        black_box(LocalRenameOptions::default as fn() -> LocalRenameOptions);
    let temp_file_default = black_box(
        LocalTempFileOptions::default as fn() -> LocalTempFileOptions,
    );
    let temp_directory_default = black_box(
        LocalTempDirectoryOptions::default as fn() -> LocalTempDirectoryOptions,
    );
    assert_eq!(create_default(), LocalCreateDirectoryOptions::new());
    assert_eq!(delete_default(), LocalDeleteOptions::new());
    assert_eq!(list_default(), LocalListOptions::new());
    assert_eq!(read_default(), LocalReadOptions::new());
    assert_eq!(copy_default(), LocalCopyOptions::new());
    assert_eq!(rename_default(), LocalRenameOptions::new());
    assert_eq!(temp_file_default(), LocalTempFileOptions::new());
    assert_eq!(temp_directory_default(), LocalTempDirectoryOptions::new());
}

/// Verifies conservative option values are observable before any builder is
/// applied.
#[test]
fn test_option_constructors_expose_conservative_values() {
    let create = black_box(LocalCreateDirectoryOptions::new as fn() -> _)();
    assert!(!black_box(LocalCreateDirectoryOptions::recursive)(&create));
    assert!(!black_box(LocalCreateDirectoryOptions::exists_ok)(&create));

    let deletion = black_box(LocalDeleteOptions::new as fn() -> _)();
    assert!(!black_box(LocalDeleteOptions::recursive)(&deletion));
    assert!(!black_box(LocalDeleteOptions::missing_ok)(&deletion));

    let listing = black_box(LocalListOptions::new as fn() -> _)();
    assert!(!black_box(LocalListOptions::recursive)(&listing));
    assert_eq!(None, black_box(LocalListOptions::symlink_policy)(&listing));
    assert_eq!(None, black_box(LocalListOptions::max_depth)(&listing));

    let copy = black_box(LocalCopyOptions::new as fn() -> _)();
    assert_eq!(
        LocalCopyConflictPolicy::Fail,
        black_box(LocalCopyOptions::conflict)(&copy)
    );
    assert_eq!(
        LocalCopyTypeConflictPolicy::Fail,
        black_box(LocalCopyOptions::type_conflict)(&copy)
    );
    assert_eq!(
        LocalMetadataPreservePolicy::None,
        black_box(LocalCopyOptions::preserve_metadata)(&copy)
    );
    assert_eq!(
        None,
        black_box(LocalCopyOptions::symlink_policy_override)(&copy)
    );
    assert_eq!(
        LocalCopySourceMode::Auto,
        black_box(LocalCopyOptions::source_mode)(&copy)
    );
    assert_eq!(
        LocalAtomicityRequirement::Preferred,
        black_box(LocalCopyOptions::atomicity)(&copy)
    );
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalCopyOptions::durability)(&copy)
    );

    let rename = black_box(LocalRenameOptions::new as fn() -> _)();
    assert!(!black_box(LocalRenameOptions::overwrite)(&rename));
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalRenameOptions::durability)(&rename)
    );

    let file = black_box(LocalTempFileOptions::default as fn() -> _)();
    assert_eq!(None, black_box(LocalTempFileOptions::parent)(&file));
    assert_eq!(None, black_box(LocalTempFileOptions::prefix)(&file));
    assert_eq!(None, black_box(LocalTempFileOptions::suffix)(&file));
    assert_eq!(256, black_box(LocalTempFileOptions::max_attempts)(&file));

    let directory =
        black_box(LocalTempDirectoryOptions::default as fn() -> _)();
    assert_eq!(
        None,
        black_box(LocalTempDirectoryOptions::parent)(&directory)
    );
    assert_eq!(
        None,
        black_box(LocalTempDirectoryOptions::prefix)(&directory)
    );
    assert_eq!(
        None,
        black_box(LocalTempDirectoryOptions::suffix)(&directory)
    );
    assert_eq!(
        256,
        black_box(LocalTempDirectoryOptions::max_attempts)(&directory)
    );

    let writer = black_box(LocalWriteOptions::new as fn(_) -> _)(
        LocalWriteMode::CreateOrReplace,
    );
    assert_eq!(
        LocalWriteMode::CreateOrReplace,
        black_box(LocalWriteOptions::mode)(&writer)
    );
    assert!(!black_box(LocalWriteOptions::creates_parent)(&writer));
    assert_eq!(
        LocalAtomicityRequirement::Preferred,
        black_box(LocalWriteOptions::atomicity)(&writer)
    );
    assert_eq!(
        LocalDurabilityRequirement::NotRequired,
        black_box(LocalWriteOptions::durability)(&writer)
    );
    assert_eq!(
        None,
        black_box(LocalWriteOptions::open_retry_timeout)(&writer)
    );
}

/// Verifies temporary-resource persistence defaults to no replacement and can
/// explicitly opt into replacement.
#[test]
fn test_persist_options_expose_overwrite_policy() {
    let conservative = black_box(LocalPersistOptions::default as fn() -> _)();
    assert!(!black_box(LocalPersistOptions::overwrites)(&conservative));

    let replacing = black_box(
        LocalPersistOptions::with_overwrite as fn(_) -> _,
    )(black_box(LocalPersistOptions::new as fn() -> _)());
    assert!(black_box(LocalPersistOptions::overwrites)(&replacing));
    let with_parent = black_box(
        LocalPersistOptions::with_create_parent as fn(_) -> _,
    )(LocalPersistOptions::new());
    assert!(black_box(LocalPersistOptions::creates_parent)(&with_parent));
    assert_eq!(LocalPersistOptions::new(), LocalPersistOptions::default());
}

/// Verifies coverage-only access to implementation option defaults and retry
/// builders that are exercised indirectly by public operations.
#[cfg(coverage)]
#[test]
fn test_internal_copy_and_atomic_option_defaults() {
    let copy = LocalCopyDirOptions::default()
        .with_open_retry_timeout(Duration::from_millis(3));
    assert_eq!(Some(Duration::from_millis(3)), copy.open_retry_timeout());

    let atomic = LocalAtomicWriteOptions::default()
        .with_open_retry_timeout(Duration::from_millis(5));
    assert_eq!(Some(Duration::from_millis(5)), atomic.open_retry_timeout());
}

/// Verifies coverage-only access to the native recursive-copy statistics.
#[cfg(coverage)]
#[test]
fn test_internal_copy_statistics_accessors() {
    let mut stats = LocalCopyDirStats::default();
    stats.files = 1;
    stats.directories = 2;
    stats.bytes = 3;
    stats.skipped = 4;
    stats.overwritten = 5;
    assert_eq!(1, stats.files());
    assert_eq!(2, stats.directories());
    assert_eq!(3, stats.bytes());
    assert_eq!(4, stats.skipped());
    assert_eq!(5, stats.overwritten());
}

/// Verifies native recursive-copy statistics are converted without loss.
#[cfg(coverage)]
#[test]
fn test_public_copy_statistics_convert_native_values() {
    let mut native = LocalCopyDirStats::default();
    native.files = 2;
    native.directories = 3;
    native.bytes = 4;
    native.skipped = 5;
    native.overwritten = 6;
    let stats = LocalCopyStats::coverage_from_internal(native);
    assert_eq!(2, stats.files());
    assert_eq!(3, stats.directories());
    assert_eq!(4, stats.bytes());
    assert_eq!(5, stats.skipped());
    assert_eq!(6, stats.overwritten());
    let skipped = LocalCopyStats::coverage_skipped_one();
    assert_eq!(1, skipped.skipped());
}

/// Verifies coverage-only construction and inspection of native copy errors.
#[cfg(coverage)]
#[test]
fn test_internal_copy_error_accessors_and_parts() {
    let source = Path::new("source").to_path_buf();
    let destination = Path::new("destination").to_path_buf();
    let error = LocalCopyDirError::coverage_new(
        LocalCopyDirStage::InspectSource,
        source.clone(),
        destination.clone(),
        LocalCopyDirStats::default(),
        std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    assert_eq!(LocalCopyDirStage::InspectSource, error.stage());
    assert_eq!(source, error.source_path());
    assert_eq!(destination, error.destination_path());
    assert_eq!(0, error.stats().files);
    assert!(error.temporary_path().is_none());
    assert!(error.cleanup_error().is_none());
    assert_eq!(std::io::ErrorKind::NotFound, error.error().kind());
    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    assert!(error.to_string().contains("source"));
    assert!(std::error::Error::source(&error).is_some());
    let (_, _, _, _, temporary, cleanup, cause) = error.coverage_into_parts();
    assert!(temporary.is_none());
    assert!(cleanup.is_none());
    assert_eq!(std::io::ErrorKind::NotFound, cause.kind());

    let contextual = LocalCopyDirError::coverage_new(
        LocalCopyDirStage::PrepareDestination,
        Path::new("source").to_path_buf(),
        Path::new("destination").to_path_buf(),
        LocalCopyDirStats::default(),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    )
    .coverage_with_staging_context(
        Path::new("temporary").to_path_buf(),
        Some(std::io::Error::from(std::io::ErrorKind::Other)),
    );
    assert!(contextual.temporary_path().is_some());
    assert!(contextual.cleanup_error().is_some());
    assert!(contextual.to_string().contains("cleanup"));
}

/// Verifies coverage-only construction and inspection of persistence errors.
#[cfg(coverage)]
#[test]
fn test_internal_persist_error_accessors_and_parts() {
    let mut error = LocalPersistError::coverage_new(
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        7_u32,
        Path::new("requested").to_path_buf(),
        Some(Path::new("resolved").to_path_buf()),
        LocalPersistStage::InstallDestination,
    );
    assert_eq!(LocalPersistFailureState::Indeterminate, error.state());
    assert_eq!(LocalPersistStage::InstallDestination, error.stage());
    assert_eq!(Path::new("requested"), error.requested_target());
    assert_eq!(Path::new("resolved"), error.resolved_target().unwrap());
    assert_eq!(qubit_local_files::LocalFileErrorKind::PermissionDenied, error.kind());
    assert_eq!(7, *error.resource());
    *error.resource_mut() = 8;
    assert_eq!(8, *error.resource());
    assert_eq!(qubit_local_files::LocalFileErrorKind::PermissionDenied, error.error().kind());
    assert!(error.to_string().contains("resolved"));
    assert!(std::error::Error::source(&error).is_some());
    let (_, resource, requested, resolved, stage, state) =
        error.into_parts_with_state();
    assert_eq!(8, resource);
    assert_eq!(Path::new("requested"), requested);
    assert_eq!(Some(Path::new("resolved").to_path_buf()), resolved);
    assert_eq!(LocalPersistStage::InstallDestination, stage);
    assert_eq!(LocalPersistFailureState::Indeterminate, state);

    let error = LocalPersistError::coverage_new(
        std::io::Error::from(std::io::ErrorKind::NotFound),
        (),
        Path::new("requested").to_path_buf(),
        None,
        LocalPersistStage::ResolveTarget,
    );
    assert!(error.resolved_target().is_none());
    assert!(error.to_string().contains("requested"));
    let (_, _, _, resolved, _) = error.into_parts();
    assert!(resolved.is_none());
}

/// Verifies coverage-only construction and inspection of atomic-write errors.
#[cfg(coverage)]
#[test]
fn test_internal_atomic_write_error_accessors_and_parts() {
    let base = || {
        LocalAtomicWriteError::coverage_new(
            LocalAtomicWriteStage::ReplaceDestination,
            Path::new("destination").to_path_buf(),
            Some(Path::new("temporary").to_path_buf()),
            LocalAtomicDestinationState::Replaced,
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        )
    };
    let error = base()
        .coverage_with_cleanup_error(Some(std::io::Error::from(
            std::io::ErrorKind::Other,
        )))
        .coverage_with_parent_sync_error(Some(std::io::Error::from(
            std::io::ErrorKind::Interrupted,
        )));
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(Path::new("destination"), error.path());
    assert_eq!(
        Some(Path::new("temporary")),
        error.temporary_path()
    );
    assert_eq!(LocalAtomicDestinationState::Replaced, error.destination_state());
    assert_eq!(std::io::ErrorKind::Other, error.cleanup_error().unwrap().kind());
    assert_eq!(
        std::io::ErrorKind::Interrupted,
        error.parent_sync_error().unwrap().kind()
    );
    assert_eq!(
        std::io::ErrorKind::PermissionDenied,
        error.source_error().kind()
    );
    assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
    assert!(error.to_string().contains("parent synchronization"));
    assert!(std::error::Error::source(&error).is_some());
    let (temporary, cleanup, source) = error.coverage_into_staging_parts();
    assert_eq!(Some(Path::new("temporary").to_path_buf()), temporary);
    assert_eq!(Some(std::io::ErrorKind::Other), cleanup.map(|e| e.kind()));
    assert_eq!(std::io::ErrorKind::PermissionDenied, source.kind());

    assert!(base().to_string().contains("atomic write"));
    assert!(base()
        .coverage_with_cleanup_error(Some(std::io::Error::from(
            std::io::ErrorKind::Other,
        )))
        .to_string()
        .contains("staging cleanup"));
    assert!(base()
        .coverage_with_parent_sync_error(Some(std::io::Error::from(
            std::io::ErrorKind::Interrupted,
        )))
        .to_string()
        .contains("parent synchronization"));
}

/// Verifies coverage-only construction and recovery accessors for commit
/// errors with and without a retained writer.
#[cfg(coverage)]
#[test]
fn test_internal_atomic_commit_error_accessors_and_parts() {
    let make_error = || {
        LocalAtomicWriteError::coverage_new(
            LocalAtomicWriteStage::ReplaceDestination,
            Path::new("destination").to_path_buf(),
            None,
            LocalAtomicDestinationState::Indeterminate,
            std::io::Error::from(std::io::ErrorKind::Other),
        )
    };

    let mut retryable = LocalAtomicCommitError::coverage_new(
        make_error(),
        Some(String::from("writer")),
    );
    assert_eq!(std::io::ErrorKind::Other, retryable.error().kind());
    assert_eq!(Some("writer"), retryable.writer().map(String::as_str));
    retryable
        .writer_mut()
        .expect("writer should be retained")
        .push_str("-updated");
    assert!(retryable.to_string().contains("retained"));
    assert!(std::error::Error::source(&retryable).is_some());
    let finalized = retryable.coverage_into_final_error_with(|writer, error| {
        assert_eq!("writer-updated", writer);
        error
    });
    assert_eq!(std::io::ErrorKind::Other, finalized.kind());

    let terminal = LocalAtomicCommitError::coverage_new(make_error(), None::<String>);
    assert!(terminal.writer().is_none());
    assert!(terminal.to_string().contains("unavailable"));
    let (error, writer) = terminal.into_parts();
    assert_eq!(std::io::ErrorKind::Other, error.kind());
    assert!(writer.is_none());
}

/// Verifies coverage-only conversion and accessors for structured copy errors.
#[cfg(coverage)]
#[test]
fn test_internal_copy_failure_conversion_and_parts() {
    let native = LocalCopyDirError::coverage_new(
        LocalCopyDirStage::PrepareDestination,
        Path::new("source").to_path_buf(),
        Path::new("destination").to_path_buf(),
        LocalCopyDirStats::default(),
        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
    )
    .coverage_with_staging_context(
        Path::new("temporary").to_path_buf(),
        Some(std::io::Error::from(std::io::ErrorKind::Other)),
    );
    let failure = LocalCopyFailure::coverage_from_copy_dir_error(
        Path::new("source"),
        Path::new("destination"),
        native,
    );
    assert_eq!(LocalCopyFailureState::Indeterminate, failure.state());
    assert_eq!(
        qubit_local_files::LocalFileErrorKind::PermissionDenied,
        failure.error().kind()
    );
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(Some(Path::new("temporary")), failure.staging_path());
    assert_eq!(
        Some(qubit_local_files::LocalFileErrorKind::Io),
        failure.cleanup_error().map(|error| error.kind())
    );
    assert!(std::error::Error::source(&failure).is_some());
    let (_, state, stats, staging, cleanup) = failure.into_parts();
    assert_eq!(LocalCopyFailureState::Indeterminate, state);
    assert_eq!(0, stats.files());
    assert_eq!(Some(Path::new("temporary").to_path_buf()), staging);
    assert!(cleanup.is_some());
}

/// Verifies coverage-only rooted permission values and Unix-mode resolution.
#[cfg(coverage)]
#[test]
fn test_internal_rooted_permissions_accessors() {
    let writable = Permissions::from_unix_mode(0o2750);
    assert!(!writable.is_read_only());
    assert_eq!(Some(0o2750), writable.unix_mode());
    assert_eq!(0o2750, writable.coverage_resolve_unix_mode(0o600));

    let read_only = Permissions::from_read_only(true);
    assert!(read_only.is_read_only());
    assert_eq!(None, read_only.unix_mode());
    assert_eq!(0o400, read_only.coverage_resolve_unix_mode(0o600));

    let writable_without_mode = Permissions::from_read_only(false);
    assert_eq!(0o700, writable_without_mode.coverage_resolve_unix_mode(0o500));
}

/// Verifies coverage-only rooted metadata accessors and Unix special kinds.
#[cfg(all(coverage, unix))]
#[test]
fn test_internal_rooted_metadata_accessors() {
    use std::os::unix::fs::MetadataExt;

    let directory = tempfile::tempdir().expect("metadata fixture directory should exist");
    let path = directory.path().join("payload");
    std::fs::write(&path, b"payload").expect("metadata fixture should be written");
    let native = std::fs::metadata(&path).expect("native metadata should be available");
    let metadata = RootedMetadata::coverage_from_native(&native);
    let mut status: libc::stat = unsafe { std::mem::zeroed() };
    status.st_mode = native.mode() as _;
    status.st_size = native.size() as _;
    status.st_atime = native.atime() as _;
    status.st_atime_nsec = native.atime_nsec() as _;
    status.st_mtime = native.mtime() as _;
    status.st_mtime_nsec = native.mtime_nsec() as _;
    status.st_ctime = native.ctime() as _;
    status.st_ctime_nsec = native.ctime_nsec() as _;
    status.st_dev = native.dev() as _;
    status.st_ino = native.ino() as _;
    let from_stat = RootedMetadata::coverage_from_stat(&status);
    assert_eq!(RootedEntryKind::File, from_stat.kind());
    assert_eq!(native.size(), from_stat.size());
    let same = RootedMetadata::coverage_from_native(&native);
    assert_eq!(RootedEntryKind::File, metadata.kind());
    assert_eq!(7, metadata.size());
    assert!(metadata.accessed_at().is_some());
    assert!(metadata.modified_at().is_some());
    assert!(metadata.created_at().is_some());
    assert!(!metadata.permissions().is_read_only());
    assert!(metadata.is_same_file(&same));
    assert!(!metadata.is_same_file(&RootedMetadata::coverage_from_native(
        &std::fs::metadata(directory.path()).unwrap(),
    )));
    assert_eq!(
        RootedEntryKind::BlockDevice,
        coverage_entry_kind_from_mode(libc::S_IFBLK as libc::mode_t),
    );
    assert_eq!(
        RootedEntryKind::CharDevice,
        coverage_entry_kind_from_mode(libc::S_IFCHR as libc::mode_t),
    );
    assert_eq!(
        RootedEntryKind::Other,
        coverage_entry_kind_from_mode(0o123 as libc::mode_t),
    );
}

/// Verifies coverage-only path-aware I/O error formatting and source access.
#[cfg(coverage)]
#[test]
fn test_internal_path_io_error_context() {
    let error = PathIoError::coverage_new(
        "inspect entry",
        Path::new("payload"),
        std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    assert!(error.to_string().contains("inspect entry"));
    assert!(error.to_string().contains("payload"));
    assert!(std::error::Error::source(&error).is_some());
}

/// Verifies context normalization preserves successes and enriches failures.
#[cfg(coverage)]
#[test]
fn test_internal_io_result_context() {
    assert_eq!(
        7,
        coverage_with_path_context(Ok::<_, std::io::Error>(7), "read", Path::new("payload"))
            .expect("successful result should remain successful"),
    );
    let error = coverage_with_path_context::<()>(
        Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        "read",
        Path::new("payload"),
    )
    .expect_err("error context should remain an error");
    assert!(error.to_string().contains("payload"));
    assert!(error.to_string().contains("read"));
}

/// Verifies coverage-only native write-open option builders and defaults.
#[cfg(coverage)]
#[test]
fn test_internal_native_write_open_options() {
    let options = NativeWriteOpenOptions::default()
        .with_parents()
        .with_open_retry_timeout(Duration::from_millis(13));
    assert_eq!(NativeWriteMode::CreateOrTruncate, options.mode());
    assert!(options.creates_parents());
    assert_eq!(Some(Duration::from_millis(13)), options.open_retry_timeout());
    assert_eq!(
        NativeWriteMode::AppendOrCreate,
        NativeWriteOpenOptions::new(NativeWriteMode::AppendOrCreate).mode()
    );
}

/// Verifies coverage-fault selector matching and consumption semantics.
#[cfg(coverage)]
#[test]
fn test_internal_coverage_fault_selectors() {
    const TEST_NAME: &str = "test_internal_coverage_fault_selectors";
    const ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(ENV).is_none() {
        let status = std::process::Command::new(
            std::env::current_exe().expect("test executable should be available"),
        )
        .arg("--exact")
        .arg(TEST_NAME)
        .arg("--nocapture")
        .env(ENV, "coverage-fault-direct")
        .status()
        .expect("coverage fault child should launch");
        assert!(status.success(), "coverage fault child should pass");
        return;
    }
    assert!(coverage_is_enabled("coverage-fault-direct"));
    assert!(!coverage_is_enabled("other-fault"));
    assert!(coverage_take("coverage-fault-direct"));
    assert!(!coverage_take("coverage-fault-direct"));
    assert!(coverage_take_on_nth("coverage-fault-direct", 1));
    assert!(coverage_take_on_nth("coverage-fault-direct", 2));
    assert!(coverage_take_on_nth("coverage-fault-direct", 3));
    assert!(!coverage_take_on_nth("coverage-fault-direct", 5));
}

/// Verifies every branch of the pure copy-destination policy matrix.
#[cfg(coverage)]
#[test]
fn test_internal_copy_destination_policy_matrix() {
    use qubit_local_files::{
        LocalCopyConflictPolicy,
        LocalCopyTypeConflictPolicy,
    };

    assert_eq!(
        Some(CoverageCopyDestinationAction::Create),
        coverage_decide_copy_destination(
            false,
            None,
            LocalCopyConflictPolicy::Fail,
            LocalCopyTypeConflictPolicy::Fail,
        )
    );
    assert_eq!(
        Some(CoverageCopyDestinationAction::Merge),
        coverage_decide_copy_destination(
            true,
            Some(true),
            LocalCopyConflictPolicy::Fail,
            LocalCopyTypeConflictPolicy::Fail,
        )
    );
    for (policy, expected) in [
        (LocalCopyTypeConflictPolicy::Fail, None),
        (
            LocalCopyTypeConflictPolicy::Replace,
            Some(CoverageCopyDestinationAction::Replace),
        ),
        (
            LocalCopyTypeConflictPolicy::Skip,
            Some(CoverageCopyDestinationAction::Skip),
        ),
    ] {
        assert_eq!(
            expected,
            coverage_decide_copy_destination(
                true,
                Some(false),
                LocalCopyConflictPolicy::Fail,
                policy,
            )
        );
    }
    for (policy, expected) in [
        (LocalCopyConflictPolicy::Fail, None),
        (
            LocalCopyConflictPolicy::Overwrite,
            Some(CoverageCopyDestinationAction::Replace),
        ),
        (
            LocalCopyConflictPolicy::Skip,
            Some(CoverageCopyDestinationAction::Skip),
        ),
    ] {
        assert_eq!(
            expected,
            coverage_decide_copy_destination(
                false,
                Some(false),
                policy,
                LocalCopyTypeConflictPolicy::Fail,
            )
        );
    }
}

/// Verifies coverage-only path-management operations across success and error
/// cases.
#[cfg(coverage)]
#[test]
fn test_internal_path_management_matrix() {
    let root = tempfile::tempdir().expect("path-management root should exist");
    let existing = root.path().join("existing");
    std::fs::create_dir(&existing).expect("existing directory should be created");
    assert!(coverage_absolute_path(Path::new("relative")).unwrap().is_absolute());
    assert_eq!(
        existing,
        coverage_canonicalize_existing_prefix(&existing).unwrap()
    );
    let missing = root.path().join("missing/tail");
    assert_eq!(
        missing,
        coverage_canonicalize_existing_prefix(&missing).unwrap()
    );
    let _ = coverage_canonicalize_existing_prefix(Path::new(""));

    let created = root.path().join("created/nested");
    coverage_ensure_dir_path(&created).expect("directory creation should succeed");
    coverage_ensure_parent_path(&root.path().join("created/file"))
        .expect("existing parent should be accepted");
    coverage_ensure_parent_path(Path::new("file"))
        .expect("a path without a parent should be accepted");
    let sync_missing = root.path().join("sync/a/b/file");
    let missing_dirs = coverage_ensure_parent_path_with_sync_dirs(&sync_missing)
        .expect("missing parents should be created");
    assert!(!missing_dirs.is_empty());
    assert!(coverage_ensure_parent_path_with_sync_dirs(&sync_missing)
        .unwrap()
        .is_empty());
    assert!(coverage_ensure_parent_path_with_sync_dirs(Path::new("file"))
        .unwrap()
        .is_empty());
    let _ = coverage_ensure_parent_path_with_sync_dirs(Path::new("/tmp/file"));
    let non_directory = root.path().join("non-directory");
    std::fs::write(&non_directory, b"file").expect("file fixture should be written");
    assert!(coverage_ensure_parent_path_with_sync_dirs(
        &non_directory.join("child")
    )
    .is_err());
    assert!(coverage_ensure_parent_path_with_sync_dirs(
        Path::new("bad\0component/child")
    )
    .is_err());

    let contextual = coverage_add_path_context(
        std::io::Error::from(std::io::ErrorKind::NotFound),
        "inspect",
        Path::new("payload"),
    );
    assert!(contextual.to_string().contains("payload"));

    let clean = root.path().join("clean");
    std::fs::create_dir(&clean).expect("clean directory should be created");
    let child = clean.join("child");
    std::fs::write(&child, b"child").expect("child should be written");
    coverage_clean_dir_path(&clean).expect("directory contents should be removed");
    assert!(clean.is_dir());
    let non_directory_child = clean.join("non-directory");
    std::fs::write(&non_directory_child, b"child")
        .expect("non-directory child should be written");
    assert!(coverage_clean_dir_path(&non_directory_child).is_err());
    let removable = root.path().join("removable");
    std::fs::write(&removable, b"remove").expect("removable file should be written");
    coverage_remove_any_path(&removable).expect("file should be removed");
    coverage_remove_any_path(&clean).expect("directory should be removed");
}
