// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private recursive directory-copy pipeline.

use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    Error,
    ErrorKind,
    Result,
};
use std::path::{
    Path,
    PathBuf,
};

#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;

use crate::{
    LocalCopyConflictPolicy,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalCopyTypeConflictPolicy,
};

use super::file_move::{
    move_file_without_replacing,
    parent_dir_for,
    replace_file,
};
use super::path_operations::canonicalize_existing_prefix;
use super::temp_entry::{
    create_private_dir,
    create_temp_file_in_dir,
};
use super::{
    DEFAULT_TEMP_FILE_RETRIES,
    StagedFile,
};

/// Prefix used by recursive-copy staging files.
const COPY_FILE_TEMP_PREFIX: &str = ".copy-file-";

/// Suffix used by recursive-copy staging files.
const COPY_FILE_TEMP_SUFFIX: &str = ".tmp";

/// Result type used by recursive directory copy internals.
type CopyDirResult<T> = std::result::Result<T, LocalCopyDirError>;

/// Builds a recursive-copy error from the current entry and statistics.
///
/// # Parameters
/// - `stage`: Stage at which the copy failed.
/// - `src`: Source entry being processed.
/// - `dst`: Destination entry being processed.
/// - `stats`: Statistics accumulated before the failure.
/// - `source`: Native I/O error that caused the failure.
///
/// # Returns
/// Structured recursive-copy error retaining the native source error.
fn copy_dir_error(
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
    source: Error,
) -> LocalCopyDirError {
    LocalCopyDirError::new(
        stage,
        src.to_path_buf(),
        dst.to_path_buf(),
        *stats,
        source,
    )
}

/// Adds recursive-copy context to a native I/O result.
///
/// # Parameters
/// - `result`: Native I/O result to convert.
/// - `stage`: Copy stage associated with the operation.
/// - `src`: Source path associated with the operation.
/// - `dst`: Destination path associated with the operation.
/// - `stats`: Statistics accumulated before the operation.
///
/// # Returns
/// The successful value or a structured recursive-copy error.
fn with_copy_context<T>(
    result: Result<T>,
    stage: LocalCopyDirStage,
    src: &Path,
    dst: &Path,
    stats: &LocalCopyDirStats,
) -> CopyDirResult<T> {
    result.map_err(|error| copy_dir_error(stage, src, dst, stats, error))
}

/// Recursively copies a directory tree with the supplied options.
///
/// # Parameters
/// - `src`: Source directory.
/// - `dst`: Destination directory.
/// - `options`: Copy behavior options.
///
/// # Returns
/// Copy statistics for regular files, created directories, and bytes.
///
/// # Errors
/// Returns a structured copy error when the source is invalid, the destination
/// is inside the source tree or a followed source directory, a followed
/// directory cycle is detected, or an underlying filesystem operation fails.
pub(crate) fn copy_dir_all_with_paths(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
) -> CopyDirResult<LocalCopyDirStats> {
    let mut active_sources = Vec::new();
    let mut stats = LocalCopyDirStats::default();
    let destination_root = with_copy_context(
        canonicalize_existing_prefix(dst),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        &stats,
    )?;
    copy_dir_recursive(
        src,
        dst,
        options,
        &destination_root,
        &mut active_sources,
        &mut stats,
    )?;
    Ok(stats)
}

/// Recursively copies one source directory into one destination directory.
///
/// # Parameters
/// - `src`: Source directory.
/// - `dst`: Destination directory.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns a structured copy error when a directory or file cannot be copied.
fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut Vec<PathBuf>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let (source_metadata, canonical_source) = with_copy_context(
        inspect_copy_source_directory(
            src,
            options.follow_symlinks,
            destination_root,
        ),
        LocalCopyDirStage::InspectSource,
        src,
        dst,
        stats,
    )?;
    if active_sources
        .iter()
        .any(|active_source| active_source == &canonical_source)
    {
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
    active_sources.push(canonical_source);
    let result = (|| {
        with_copy_context(
            ensure_copy_destination_dir(dst, options.type_conflict, stats),
            LocalCopyDirStage::PrepareDestination,
            src,
            dst,
            stats,
        )?;
        let entries = with_copy_context(
            fs::read_dir(src),
            LocalCopyDirStage::ReadSourceDirectory,
            src,
            dst,
            stats,
        )?;
        for entry in entries {
            let entry = with_copy_context(
                entry,
                LocalCopyDirStage::ReadSourceDirectory,
                src,
                dst,
                stats,
            )?;
            let source_path = entry.path();
            let destination_path = dst.join(entry.file_name());
            let file_type = with_copy_context(
                entry.file_type(),
                LocalCopyDirStage::InspectSourceEntry,
                &source_path,
                &destination_path,
                stats,
            )?;
            if file_type.is_symlink() {
                copy_symlink_source(
                    &source_path,
                    &destination_path,
                    options,
                    destination_root,
                    active_sources,
                    stats,
                )?;
            } else if file_type.is_dir() {
                copy_dir_recursive(
                    &source_path,
                    &destination_path,
                    options,
                    destination_root,
                    active_sources,
                    stats,
                )?;
            } else if file_type.is_file() {
                copy_file_with_options(
                    &source_path,
                    &destination_path,
                    options,
                    stats,
                )?;
            } else {
                return Err(copy_dir_error(
                    LocalCopyDirStage::InspectSourceEntry,
                    &source_path,
                    &destination_path,
                    stats,
                    Error::new(
                        ErrorKind::Unsupported,
                        format!(
                            "unsupported source file type: {}",
                            source_path.display()
                        ),
                    ),
                ));
            }
        }
        if options.preserve_permissions {
            with_copy_context(
                fs::set_permissions(dst, source_metadata.permissions()),
                LocalCopyDirStage::PreservePermissions,
                src,
                dst,
                stats,
            )?;
        }
        Ok(())
    })();
    let _ = active_sources.pop();
    result
}

/// Copies a symbolic link source when link following is enabled.
///
/// # Parameters
/// - `src`: Source symbolic link.
/// - `dst`: Destination path.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns a structured copy error when symbolic links are disabled or the
/// target cannot be copied.
fn copy_symlink_source(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    destination_root: &Path,
    active_sources: &mut Vec<PathBuf>,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    if !options.follow_symlinks {
        return Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!("symbolic links are not followed: {}", src.display()),
            ),
        ));
    }
    let target_metadata = with_copy_context(
        fs::metadata(src),
        LocalCopyDirStage::InspectSourceEntry,
        src,
        dst,
        stats,
    )?;
    if target_metadata.is_dir() {
        copy_dir_recursive(
            src,
            dst,
            options,
            destination_root,
            active_sources,
            stats,
        )
    } else if target_metadata.is_file() {
        copy_file_with_options(src, dst, options, stats)
    } else {
        Err(copy_dir_error(
            LocalCopyDirStage::InspectSourceEntry,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::Unsupported,
                format!(
                    "unsupported symbolic link target type: {}",
                    src.display()
                ),
            ),
        ))
    }
}

/// Inspects a source directory before recursive copy enters it.
///
/// # Parameters
/// - `src`: Source directory path.
/// - `follow_symlinks`: Whether symbolic links may be followed.
/// - `destination_root`: Canonical destination root, including missing tail
///   components.
///
/// # Returns
/// Source metadata and canonical source directory path.
///
/// # Errors
/// Returns an I/O error when `src` is not a directory, cannot be canonicalized,
/// or would contain the destination root.
fn inspect_copy_source_directory(
    src: &Path,
    follow_symlinks: bool,
    destination_root: &Path,
) -> Result<(fs::Metadata, PathBuf)> {
    let source_metadata = metadata_for_copy_source(src, follow_symlinks)?;
    if !source_metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    let canonical_source = fs::canonicalize(src)?;
    reject_destination_inside_source(src, &canonical_source, destination_root)?;
    Ok((source_metadata, canonical_source))
}

/// Ensures a directory copy destination exists as a directory.
///
/// # Parameters
/// - `dst`: Destination directory path.
/// - `type_conflict`: Policy for an existing non-directory destination.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns an I/O error when the destination cannot be created or cannot be
/// replaced according to `type_conflict`.
fn ensure_copy_destination_dir(
    dst: &Path,
    type_conflict: LocalCopyTypeConflictPolicy,
    stats: &mut LocalCopyDirStats,
) -> Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(metadata) => {
            if is_real_directory(&metadata) {
                return Ok(());
            }
            match type_conflict {
                LocalCopyTypeConflictPolicy::Fail => {
                    return Err(Error::new(
                        ErrorKind::AlreadyExists,
                        format!(
                            "destination type conflicts with source directory: {}",
                            dst.display()
                        ),
                    ));
                }
                LocalCopyTypeConflictPolicy::Replace => {
                    remove_destination_non_directory_if_unchanged(dst)?;
                }
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match create_private_dir(dst) {
        Ok(()) => {
            stats.directories = stats.directories.saturating_add(1);
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(dst)?;
            if is_real_directory(&metadata) {
                Ok(())
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

/// Removes a non-directory destination only while its observed type is stable.
///
/// A real directory that appears after the caller's earlier inspection is
/// retained and treated as a mergeable directory destination. File-specific
/// removal APIs prevent a concurrently substituted real directory from being
/// recursively deleted.
///
/// # Arguments
///
/// * `dst` - Destination previously observed as a non-directory entry.
///
/// # Errors
///
/// Returns the I/O error reported while inspecting or removing `dst`.
fn remove_destination_non_directory_if_unchanged(dst: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(dst) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_real_directory(&metadata) {
        return Ok(());
    }
    #[cfg(windows)]
    if metadata.file_type().is_symlink_dir() {
        fs::remove_dir(dst)?;
        return Ok(());
    }
    fs::remove_file(dst)
}

/// Removes a directory destination only while it remains a real directory.
///
/// A non-directory entry that appears after the caller's earlier inspection is
/// retained for the file-conflict policy to handle during commit.
///
/// # Arguments
///
/// * `dst` - Destination previously observed as a real directory.
///
/// # Errors
///
/// Returns the I/O error reported while inspecting or recursively removing the
/// directory.
fn remove_destination_directory_if_unchanged(dst: &Path) -> Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(metadata) if is_real_directory(&metadata) => fs::remove_dir_all(dst),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Copies one regular source file into a destination path.
///
/// # Parameters
/// - `src`: Source file path.
/// - `dst`: Destination file path.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns a structured copy error when the destination conflict policy rejects
/// the copy or the file cannot be staged and committed.
fn copy_file_with_options(
    src: &Path,
    dst: &Path,
    options: LocalCopyDirOptions,
    stats: &mut LocalCopyDirStats,
) -> CopyDirResult<()> {
    let source_metadata = with_copy_context(
        metadata_for_copy_source(src, options.follow_symlinks),
        LocalCopyDirStage::InspectSourceEntry,
        src,
        dst,
        stats,
    )?;
    let destination_metadata = with_copy_context(
        match fs::symlink_metadata(dst) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        },
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let destination_directory_requires_removal = match destination_metadata {
        Some(metadata) if is_real_directory(&metadata) => {
            match options.type_conflict {
                LocalCopyTypeConflictPolicy::Fail => {
                    return Err(copy_dir_error(
                        LocalCopyDirStage::PrepareDestination,
                        src,
                        dst,
                        stats,
                        Error::new(
                            ErrorKind::AlreadyExists,
                            format!(
                                "destination type conflicts with source file: {}",
                                dst.display()
                            ),
                        ),
                    ));
                }
                LocalCopyTypeConflictPolicy::Replace => true,
            }
        }
        Some(_) => match options.conflict {
            LocalCopyConflictPolicy::Fail => {
                return Err(copy_dir_error(
                    LocalCopyDirStage::PrepareDestination,
                    src,
                    dst,
                    stats,
                    Error::new(
                        ErrorKind::AlreadyExists,
                        format!(
                            "destination already exists: {}",
                            dst.display()
                        ),
                    ),
                ));
            }
            LocalCopyConflictPolicy::Skip => {
                stats.skipped = stats.skipped.saturating_add(1);
                return Ok(());
            }
            LocalCopyConflictPolicy::Overwrite => false,
        },
        None => false,
    };

    let (temp_path, temp_file) = with_copy_context(
        create_temp_file_in_dir(
            parent_dir_for(dst),
            Some(COPY_FILE_TEMP_PREFIX),
            Some(COPY_FILE_TEMP_SUFFIX),
            DEFAULT_TEMP_FILE_RETRIES,
        ),
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
    )?;
    let mut staged_file = StagedFile::new(temp_path, temp_file);
    let copied = with_copy_context(
        File::open(src).and_then(|mut source_file| {
            io::copy(&mut source_file, staged_file.file_mut())
        }),
        LocalCopyDirStage::CopyFileContents,
        src,
        dst,
        stats,
    )?;
    if options.preserve_permissions {
        with_copy_context(
            staged_file
                .file()
                .set_permissions(source_metadata.permissions()),
            LocalCopyDirStage::PreservePermissions,
            src,
            dst,
            stats,
        )?;
    }
    staged_file.close();

    if destination_directory_requires_removal {
        // Stage the source before deleting a conflicting directory so read
        // failures cannot destroy the existing destination. Re-check the
        // entry type and use directory-only removal so a racing file is left
        // for the configured file-conflict policy. Removing a directory and
        // then moving a file cannot be one atomic filesystem operation, so
        // commit failure after this point may still leave the destination
        // absent.
        with_copy_context(
            remove_destination_directory_if_unchanged(dst),
            LocalCopyDirStage::PrepareDestination,
            src,
            dst,
            stats,
        )?;
    }

    let commit_result = match options.conflict {
        LocalCopyConflictPolicy::Fail | LocalCopyConflictPolicy::Skip => {
            move_file_without_replacing(staged_file.path(), dst)
        }
        LocalCopyConflictPolicy::Overwrite => {
            replace_file(staged_file.path(), dst)
        }
    };
    match commit_result {
        Ok(()) => {}
        Err(error)
            if options.conflict == LocalCopyConflictPolicy::Skip
                && error.kind() == ErrorKind::AlreadyExists =>
        {
            stats.skipped = stats.skipped.saturating_add(1);
            return Ok(());
        }
        Err(error) => {
            return with_copy_context(
                Err(error),
                LocalCopyDirStage::CommitFile,
                src,
                dst,
                stats,
            );
        }
    }
    staged_file.disarm();

    stats.files = stats.files.saturating_add(1);
    stats.bytes = stats.bytes.saturating_add(copied);
    Ok(())
}

/// Loads metadata for a source path according to symlink policy.
///
/// # Parameters
/// - `path`: Source path.
/// - `follow_symlinks`: Whether symbolic links may be followed.
///
/// # Returns
/// Metadata for `path`, following a symbolic link when allowed.
///
/// # Errors
/// Returns an I/O error when metadata cannot be loaded or a symbolic link is
/// encountered while `follow_symlinks` is `false`.
fn metadata_for_copy_source(
    path: &Path,
    follow_symlinks: bool,
) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        if follow_symlinks {
            fs::metadata(path)
        } else {
            Err(Error::new(
                ErrorKind::Unsupported,
                format!("symbolic links are not followed: {}", path.display()),
            ))
        }
    } else {
        Ok(metadata)
    }
}

/// Tests whether metadata describes a real directory rather than a symlink.
///
/// Both metadata predicates are side-effect free, so evaluating both avoids
/// duplicating platform-sensitive directory checks throughout the copy
/// pipeline.
///
/// # Parameters
/// - `metadata`: Metadata loaded without following the final path component.
///
/// # Returns
/// `true` only for a non-symlink directory.
#[inline(always)]
fn is_real_directory(metadata: &fs::Metadata) -> bool {
    metadata.is_dir() & !metadata.file_type().is_symlink()
}

/// Rejects copy destinations located inside the source tree.
///
/// # Parameters
/// - `src`: Source directory.
/// - `canonical_source`: Canonical source directory path.
/// - `destination`: Canonical destination root, including missing tail
///   components.
///
/// # Errors
/// Returns an I/O error when `destination` is equal to or nested under
/// `canonical_source`.
fn reject_destination_inside_source(
    src: &Path,
    canonical_source: &Path,
    destination: &Path,
) -> Result<()> {
    if destination == canonical_source
        || destination.starts_with(canonical_source)
    {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "destination must not be inside source: source={}, destination={}",
                src.display(),
                destination.display(),
            ),
        ));
    }
    Ok(())
}
