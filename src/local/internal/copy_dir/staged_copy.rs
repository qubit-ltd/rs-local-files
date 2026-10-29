// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regular-file staging and commit for recursive directory copies.
// Private behavior is covered through public integration tests.

use std::io::ErrorKind;
#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;
use std::path::Path;

use super::copy_dir_result::CopyDirResult;
use super::destination::destination_metadata_if_exists;
use super::destination::existing_file_destination_should_be_skipped;
use super::destination::remove_destination_directory_if_unchanged;
use super::error::copy_dir_error;
use super::error::copy_dir_error_with_staging;
use super::error::record_copied_file;
use super::error::record_overwritten_entry;
use super::error::record_skipped_file;
use super::error::with_copy_context;
use super::opened_copy_source::OpenedCopySource;
use super::source::is_real_directory;
use super::staging_io::copy_into_staging;
use super::staging_io::preserve_staged_permissions;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::LocalDurabilityRequirement;
use crate::LocalSymlinkPolicy;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;
use crate::local::decide_copy_destination;
use crate::local::internal::StagedFile;
use crate::local::internal::file_move::move_file_without_replacing;
use crate::local::internal::file_move::parent_dir_for;
use crate::local::internal::file_move::replace_file;
use crate::local::internal::temp_entry::create_temp_file_in_dir;

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
/// * `budget` - Shared resource state for the complete copy.
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
    budget: &mut CopyBudget,
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
            let action = decide_copy_destination(
                false,
                Some(true),
                options.conflict_policy(),
                options.type_conflict_policy(),
            );
            match action {
                Some(CopyDestinationAction::Replace) => true,
                Some(CopyDestinationAction::Skip) => {
                    return with_copy_context(
                        record_skipped_file(stats),
                        LocalCopyDirStage::UpdateStatistics,
                        src,
                        dst,
                        stats,
                    );
                }
                _ => {
                    return Err(copy_dir_error(
                        LocalCopyDirStage::PrepareDestination,
                        src,
                        dst,
                        stats,
                        std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            format!("destination type conflicts with source file: {}", dst.display(),),
                        ),
                    ));
                }
            }
        }
        Some(_) => {
            if existing_file_destination_should_be_skipped(src, dst, options.conflict_policy(), stats)? {
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

    let (staged_file, copied, file_durable) = stage_copy_file(src, dst, options, stats, budget)?;
    if !commit_staged_copy_file(
        src,
        dst,
        StagedCopyCommitOptions {
            conflict: options.conflict_policy(),
            remove_destination_directory: destination_directory_requires_removal,
            symlink_policy: options.symlink_policy(),
        },
        stats,
        staged_file,
        budget,
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
    stats.files_durable &= file_durable;
    Ok(())
}

/// Copies a symbolic-link entry without dereferencing its final target.
///
/// Applies destination conflict policy and counts a copied link as one file
/// with zero bytes. Returns inspection, removal, link-read, publication, or
/// accounting errors. A removed conflicting destination is not restored when
/// a later step fails.
pub(crate) fn copy_symlink_with_options(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    stats: &mut LocalCopyDirStats,
    budget: &CopyBudget,
) -> CopyDirResult<()> {
    let destination_metadata = with_copy_context(
        destination_metadata_if_exists(dst),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let action = decide_copy_destination(
        false,
        destination_metadata.as_ref().map(is_real_directory),
        options.conflict_policy(),
        options.type_conflict_policy(),
    );
    let Some(action) = action else {
        return Err(copy_dir_error(
            LocalCopyDirStage::PrepareDestination,
            src,
            dst,
            stats,
            std::io::Error::from(ErrorKind::AlreadyExists),
        ));
    };
    if action == CopyDestinationAction::Skip {
        return with_copy_context(
            record_skipped_file(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        );
    }
    let link_target = with_copy_context(
        std::fs::read_link(src),
        LocalCopyDirStage::CopyFileContents,
        src,
        dst,
        stats,
    )?;
    if action == CopyDestinationAction::Replace {
        let (delete_options, started_at) = budget.deletion_options();
        let removal = match destination_metadata.as_ref() {
            Some(metadata) if is_real_directory(metadata) => {
                remove_destination_directory_if_unchanged(dst, &delete_options, options.symlink_policy(), started_at)
            }
            #[cfg(windows)]
            Some(metadata) if metadata.file_type().is_symlink_dir() => {
                crate::local::internal::file_move::remove_directory_symlink(dst).map_err(|error| (error, None))
            }
            _ => std::fs::remove_file(dst).map_err(|error| (error, None)),
        };
        removal.map_err(|(source, state)| {
            let error = copy_dir_error(LocalCopyDirStage::PrepareDestination, src, dst, stats, source);
            if let Some(state) = state {
                error.with_failure_state(state)
            } else {
                error
            }
        })?;
    }
    with_copy_context(
        create_symlink_entry(&link_target, src, dst),
        LocalCopyDirStage::CommitFile,
        src,
        dst,
        stats,
    )?;
    with_copy_context(
        record_copied_file(stats, 0),
        LocalCopyDirStage::UpdateStatistics,
        src,
        dst,
        stats,
    )?;
    if destination_metadata.is_some() {
        with_copy_context(
            record_overwritten_entry(stats),
            LocalCopyDirStage::UpdateStatistics,
            src,
            dst,
            stats,
        )?;
    }
    stats.non_atomic_publication = true;
    stats.files_durable = false;
    Ok(())
}

#[cfg(test)]
mod symlink_source_read_tests {
    use super::CopyBudget;
    use super::LocalCopyConflictPolicy;
    use super::LocalCopyDirOptions;
    use super::LocalCopyDirStage;
    use super::LocalCopyDirStats;
    use super::copy_symlink_with_options;
    use crate::LocalCopyTypeConflictPolicy;

    #[test]
    fn test_symlink_source_read_failure_keeps_destination() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("ordinary-file");
        std::fs::write(&source, b"not a link").expect("ordinary source file");
        let options = LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Replace);

        for directory_destination in [false, true] {
            let destination = fixture
                .path()
                .join(if directory_destination { "old-dir" } else { "old-file" });
            if directory_destination {
                std::fs::create_dir(&destination).expect("old destination directory");
                std::fs::write(destination.join("old"), b"old child").expect("old child");
            } else {
                std::fs::write(&destination, b"old").expect("old destination file");
            }
            let mut stats = LocalCopyDirStats::default();
            let before = stats;
            let budget = CopyBudget::new(options);
            let failure = copy_symlink_with_options(&source, &destination, options, &mut stats, &budget)
                .expect_err("ordinary source is not a link");

            assert_eq!(LocalCopyDirStage::CopyFileContents, failure.stage());
            assert_eq!(before, stats);
            if directory_destination {
                assert_eq!(
                    b"old child",
                    std::fs::read(destination.join("old"))
                        .expect("old child retained")
                        .as_slice()
                );
            } else {
                assert_eq!(
                    b"old",
                    std::fs::read(&destination).expect("old file retained").as_slice()
                );
            }
        }

        for (conflict, expected_stage) in [
            (
                LocalCopyConflictPolicy::Fail,
                Some(LocalCopyDirStage::PrepareDestination),
            ),
            (LocalCopyConflictPolicy::Skip, None),
        ] {
            let destination = fixture.path().join("existing");
            std::fs::write(&destination, b"old").expect("existing destination");
            let options = LocalCopyDirOptions::new().with_conflict(conflict);
            let mut stats = LocalCopyDirStats::default();
            let budget = CopyBudget::new(options);
            match expected_stage {
                Some(stage) => {
                    let failure = copy_symlink_with_options(&source, &destination, options, &mut stats, &budget)
                        .expect_err("Fail policy rejects existing destination");
                    assert_eq!(stage, failure.stage());
                    assert_eq!(0, stats.skipped());
                }
                None => {
                    copy_symlink_with_options(&source, &destination, options, &mut stats, &budget)
                        .expect("Skip policy does not inspect source link");
                    assert_eq!(1, stats.skipped());
                }
            }
            assert_eq!(
                b"old",
                std::fs::read(&destination).expect("destination retained").as_slice()
            );
        }
    }
}

/// Creates a symbolic link at `target` using the source entry type where the
/// native platform requires that distinction.
///
/// # Parameters
///
/// * `link_target` - Link target stored in the new directory entry.
/// * `_source` - Source entry used to determine the Windows link kind.
/// * `target` - Destination link path.
///
/// # Errors
///
/// Returns source-type inspection or native link-creation errors, or
/// `Unsupported` on platforms that do not expose symbolic links.
fn create_symlink_entry(link_target: &Path, _source: &Path, target: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link_target, target)
    }
    #[cfg(windows)]
    {
        if std::fs::symlink_metadata(_source)?.file_type().is_symlink_dir() {
            std::os::windows::fs::symlink_dir(link_target, target)
        } else {
            std::os::windows::fs::symlink_file(link_target, target)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (link_target, _source, target);
        Err(std::io::Error::new(
            ErrorKind::Unsupported,
            "symbolic links are unsupported on this platform",
        ))
    }
}

/// Copies a source file into a private same-directory staging file.
///
/// # Parameters
///
/// * `src` - Regular source file path.
/// * `dst` - Final destination file path.
/// * `options` - Recursive-copy behavior options.
/// * `stats` - Statistics accumulated before staging.
/// * `budget` - Shared byte and deadline budget charged while copying.
///
/// # Returns
///
/// An armed guard with its file handle closed, the copied byte count, and
/// whether staging-file synchronization succeeded. The durability flag does
/// not include the later destination-parent synchronization.
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
    budget: &mut CopyBudget,
) -> CopyDirResult<(StagedFile, u64, bool)> {
    let (temp_path, temp_file) = with_copy_context(
        create_temp_file_in_dir(
            parent_dir_for(dst),
            Some(COPY_FILE_TEMP_PREFIX),
            Some(COPY_FILE_TEMP_SUFFIX),
            None,
        ),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let mut staged_file = StagedFile::new(temp_path, temp_file);
    let opened_source = match OpenedCopySource::open(src, options.symlink_policy(), options.open_retry_timeout()) {
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
    let copied = copy_into_staging(src, dst, stats, &mut source_file, &mut staged_file, budget)?;
    if options.preserves_permissions() {
        preserve_staged_permissions(src, dst, &source_metadata, stats, &mut staged_file)?;
    }
    let file_durable = match options.durability() {
        LocalDurabilityRequirement::NotRequired => false,
        LocalDurabilityRequirement::Preferred => sync_staged_file(&staged_file).is_ok(),
        LocalDurabilityRequirement::Required => {
            if let Err(source) = sync_staged_file(&staged_file) {
                return Err(copy_dir_error_with_staging(
                    LocalCopyDirStage::SynchronizeFile,
                    src,
                    dst,
                    stats,
                    source,
                    &mut staged_file,
                ));
            }
            true
        }
    };
    staged_file.close();
    Ok((staged_file, copied, file_durable))
}

/// Synchronizes staged file data before its namespace publication.
///
/// Returns native synchronization errors. The caller must keep the staging
/// handle open; accessing a closed handle panics.
fn sync_staged_file(staged_file: &StagedFile) -> std::io::Result<()> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("copy-staging-file-sync") {
        return Err(crate::local::test_fault_error());
    }
    staged_file.file().sync_all()
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
struct StagedCopyCommitOptions {
    conflict: LocalCopyConflictPolicy,
    remove_destination_directory: bool,
    symlink_policy: LocalSymlinkPolicy,
}

fn commit_staged_copy_file(
    src: &Path,
    dst: &Path,
    options: StagedCopyCommitOptions,
    stats: &LocalCopyDirStats,
    mut staged_file: StagedFile,
    budget: &CopyBudget,
) -> CopyDirResult<bool> {
    if options.remove_destination_directory {
        // Stage source bytes before deleting a conflicting directory. The
        // second type check ensures a racing file is left for file policy.
        let (delete_options, started_at) = budget.deletion_options();
        if let Err((source, state)) =
            remove_destination_directory_if_unchanged(dst, &delete_options, options.symlink_policy, started_at)
        {
            let error = copy_dir_error_with_staging(
                LocalCopyDirStage::PrepareDestination,
                src,
                dst,
                stats,
                source,
                &mut staged_file,
            );
            return Err(if let Some(state) = state {
                error.with_failure_state(state)
            } else {
                error
            });
        }
    }

    let commit_result = match options.conflict {
        LocalCopyConflictPolicy::Fail | LocalCopyConflictPolicy::Skip => {
            move_file_without_replacing(staged_file.path(), dst)
        }
        LocalCopyConflictPolicy::Overwrite => replace_file(staged_file.path(), dst),
    };
    match commit_result {
        Ok(()) => {
            staged_file.disarm();
            Ok(true)
        }
        Err(error) if options.conflict == LocalCopyConflictPolicy::Skip && error.kind() == ErrorKind::AlreadyExists => {
            let temporary_path = staged_file.path().to_path_buf();
            if let Err(source) = staged_file.cleanup() {
                return Err(
                    copy_dir_error(LocalCopyDirStage::CleanupTemporaryFile, src, dst, stats, source)
                        .with_staging_context(temporary_path, None),
                );
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

#[cfg(all(test, unix))]
mod error_contract_tests {
    use super::*;

    #[test]
    fn racing_destination_skip_cleans_real_staging_and_keeps_winner() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let destination = fixture.path().join("destination");
        std::fs::write(&source, b"candidate").expect("source");
        let options = LocalCopyDirOptions::new().with_conflict(LocalCopyConflictPolicy::Skip);
        let stats = LocalCopyDirStats::default();
        let mut budget = CopyBudget::new(options);
        let (staged, copied, _) = stage_copy_file(&source, &destination, options, &stats, &mut budget)
            .expect("source staged before competing actor");
        assert_eq!(copied, 9);
        let staging_path = staged.path().to_path_buf();
        std::fs::write(&destination, b"winner").expect("racing publisher");
        let committed = commit_staged_copy_file(
            &source,
            &destination,
            StagedCopyCommitOptions {
                conflict: LocalCopyConflictPolicy::Skip,
                remove_destination_directory: false,
                symlink_policy: LocalSymlinkPolicy::Reject,
            },
            &stats,
            staged,
            &budget,
        )
        .expect("racing winner skipped");
        assert!(!committed);
        assert!(!staging_path.exists());
        assert_eq!(std::fs::read(destination).expect("winner intact"), b"winner");
        assert_eq!(std::fs::read(source).expect("source intact"), b"candidate");
    }

    #[test]
    fn expired_replacement_budget_keeps_destination_tree_and_cleans_staging() {
        let fixture = tempfile::tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let destination = fixture.path().join("destination");
        std::fs::write(&source, b"candidate").expect("source");
        std::fs::create_dir(&destination).expect("destination directory");
        std::fs::write(destination.join("kept"), b"old").expect("old child");
        let options = LocalCopyDirOptions::new();
        let stats = LocalCopyDirStats::default();
        let mut staging_budget = CopyBudget::new(options);
        let (staged, _, _) = stage_copy_file(&source, &destination, options, &stats, &mut staging_budget)
            .expect("source staged before deadline");
        let staging_path = staged.path().to_path_buf();
        let expired = CopyBudget::new(options.with_deadline(std::time::Duration::ZERO));
        let error = commit_staged_copy_file(
            &source,
            &destination,
            StagedCopyCommitOptions {
                conflict: LocalCopyConflictPolicy::Overwrite,
                remove_destination_directory: true,
                symlink_policy: LocalSymlinkPolicy::Reject,
            },
            &stats,
            staged,
            &expired,
        )
        .expect_err("replacement deletion deadline expired");
        assert_eq!(error.stage(), LocalCopyDirStage::PrepareDestination);
        assert_eq!(error.error().kind(), ErrorKind::TimedOut);
        assert_eq!(error.source_path(), source);
        assert_eq!(error.destination_path(), destination);
        assert_eq!(std::fs::read(destination.join("kept")).expect("tree preserved"), b"old");
        assert!(!staging_path.exists());
    }
}
