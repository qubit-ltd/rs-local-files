// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regular-file staging and commit for recursive directory copies.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::io::ErrorKind;
use std::path::Path;

use crate::{
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
};

use crate::local::internal::StagedFile;
use crate::local::internal::file_move::{
    move_file_without_replacing,
    parent_dir_for,
    replace_file,
};
use crate::local::internal::temp_entry::{
    DEFAULT_TEMP_ENTRY_RETRIES,
    create_temp_file_in_dir,
};

use super::copy_dir_result::CopyDirResult;
use super::destination::{
    destination_metadata_if_exists,
    ensure_directory_can_be_replaced_by_file,
    existing_file_destination_should_be_skipped,
    remove_destination_directory_if_unchanged,
};
use super::error::{
    copy_dir_error,
    copy_dir_error_with_staging,
    record_copied_file,
    record_overwritten_entry,
    record_skipped_file,
    with_copy_context,
};
use super::opened_copy_source::OpenedCopySource;
use super::source::is_real_directory;
use super::staging_io::{
    copy_into_staging,
    preserve_staged_permissions,
};

/// Prefix used by recursive-copy staging files.
const COPY_FILE_TEMP_PREFIX: &str = ".copy-file-";

/// Suffix used by recursive-copy staging files.
const COPY_FILE_TEMP_SUFFIX: &str = ".tmp";

/// Copies one regular source file into a destination path.
///
/// # Parameters
///
/// * `src` - Source file path.
/// * `dst` - Destination file path.
/// * `options` - Recursive-copy behavior options.
/// * `stats` - Mutable statistics accumulator.
///
/// # Errors
///
/// Returns a structured error when policy rejects the destination, staging or
/// commit fails, or exact statistics cannot be represented.
pub(crate) fn copy_file_with_options(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let destination_metadata = with_copy_context(
        destination_metadata_if_exists(dst),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let destination_existed = destination_metadata.is_some();
    let destination_directory_requires_removal = match destination_metadata {
        Some(metadata) if is_real_directory(&metadata) => {
            ensure_directory_can_be_replaced_by_file(
                src,
                dst,
                options.type_conflict_policy(),
                stats,
            )?;
            true
        }
        Some(_) => {
            if existing_file_destination_should_be_skipped(
                src,
                dst,
                options.conflict_policy(),
                stats,
            )? {
                return with_copy_context(
                    record_skipped_file(stats),
                    LocalCopyDirStage::UpdateStatistics,
                    src,
                    dst,
                    stats,
                );
            }
            false
        }
        None => false,
    };

    let (staged_file, copied) = stage_copy_file(src, dst, options, stats)?;
    if !commit_staged_copy_file(
        src,
        dst,
        options.conflict_policy(),
        destination_directory_requires_removal,
        stats,
        staged_file,
    )? {
        return with_copy_context(
            record_skipped_file(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        );
    }

    with_copy_context(
        record_copied_file(stats, copied),
        LocalCopyDirStage::UpdateStatistics,
        src,
        dst,
        stats,
    )?;
    if destination_existed {
        with_copy_context(
            record_overwritten_entry(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    if destination_directory_requires_removal {
        stats.non_atomic_publication = true;
    }
    Ok(())
}

/// Copies a source file into a private same-directory staging file.
///
/// # Parameters
///
/// * `src` - Regular source file path.
/// * `dst` - Final destination file path.
/// * `options` - Recursive-copy behavior options.
/// * `stats` - Statistics accumulated before staging.
///
/// # Returns
///
/// An armed staging guard and the number of source bytes copied.
///
/// # Errors
///
/// Returns a structured error when staging creation, copying, permission
/// preservation, or failure cleanup fails.
fn stage_copy_file(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    stats: &LocalCopyDirStats,
) -> CopyDirResult<(StagedFile, u64)> {
    let (temp_path, temp_file) = with_copy_context(
        create_temp_file_in_dir(
            parent_dir_for(dst),
            Some(COPY_FILE_TEMP_PREFIX),
            Some(COPY_FILE_TEMP_SUFFIX),
            DEFAULT_TEMP_ENTRY_RETRIES,
        ),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let mut staged_file = StagedFile::new(temp_path, temp_file);
    let opened_source = match OpenedCopySource::open(
        src,
        options.follows_symlinks(),
        options.open_retry_timeout(),
    ) {
        Ok(source) => source,
        Err(source) => {
            return Err(copy_dir_error_with_staging(
                LocalCopyDirStage::CopyFileContents,
                src,
                dst,
                stats,
                source,
                &mut staged_file,
            ));
        }
    };
    let (mut source_file, source_metadata) = opened_source.into_parts();
    let copied =
        copy_into_staging(src, dst, stats, &mut source_file, &mut staged_file)?;
    if options.preserves_permissions() {
        preserve_staged_permissions(
            src,
            dst,
            &source_metadata,
            stats,
            &mut staged_file,
        )?;
    }
    staged_file.close();
    Ok((staged_file, copied))
}

/// Commits an already staged regular file according to destination policies.
///
/// # Parameters
///
/// * `src` - Source file associated with the staged contents.
/// * `dst` - Final destination file path.
/// * `conflict` - Existing-file conflict policy.
/// * `remove_destination_directory` - Whether a prior directory conflict may be
///   removed immediately before commit.
/// * `stats` - Statistics accumulated before commit.
/// * `staged_file` - Armed staging guard to commit or clean up.
///
/// # Returns
///
/// `true` when committed, or `false` when a racing destination was skipped.
///
/// # Errors
///
/// Returns a structured error when destination removal, commit, or staging
/// cleanup fails.
fn commit_staged_copy_file(
    src: &Path,
    dst: &Path,
    conflict: LocalCopyConflictPolicy,
    remove_destination_directory: bool,
    stats: &LocalCopyDirStats,
    mut staged_file: StagedFile,
) -> CopyDirResult<bool> {
    if remove_destination_directory {
        // Stage source bytes before deleting a conflicting directory. The
        // second type check ensures a racing file is left for file policy.
        if let Err(source) = remove_destination_directory_if_unchanged(dst) {
            return Err(copy_dir_error_with_staging(
                LocalCopyDirStage::PrepareDestination,
                src,
                dst,
                stats,
                source,
                &mut staged_file,
            ));
        }
    }

    let commit_result = match conflict {
        LocalCopyConflictPolicy::Fail | LocalCopyConflictPolicy::Skip => {
            move_file_without_replacing(staged_file.path(), dst)
        }
        LocalCopyConflictPolicy::Overwrite => {
            replace_file(staged_file.path(), dst)
        }
    };
    match commit_result {
        Ok(()) => {
            staged_file.disarm();
            Ok(true)
        }
        Err(error)
            if conflict == LocalCopyConflictPolicy::Skip
                && error.kind() == ErrorKind::AlreadyExists =>
        {
            let temporary_path = staged_file.path().to_path_buf();
            if let Err(source) = staged_file.cleanup() {
                return Err(copy_dir_error(
                    LocalCopyDirStage::CleanupTemporaryFile,
                    src,
                    dst,
                    stats,
                    source,
                )
                .with_staging_context(temporary_path, None));
            }
            Ok(false)
        }
        Err(error) => Err(copy_dir_error_with_staging(
            LocalCopyDirStage::CommitFile,
            src,
            dst,
            stats,
            error,
            &mut staged_file,
        )),
    }
}
