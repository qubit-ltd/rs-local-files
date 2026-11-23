// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative file and directory copying.
mod copy_frame;
mod destination;
mod file;
mod rooted_copy_backend;
mod symlink;

use std::io;
use std::io::ErrorKind;

use destination::error;
use destination::unsupported_source_error;
use file::copy_file;
use rooted_copy_backend::copy_tree;
use symlink::copy_symlink;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use crate::LocalCopyFailureState;
use crate::LocalDurabilityRequirement;
use crate::LocalFileError;
use crate::error::LocalFileEffectState;
use crate::local::CopyBudget;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;

/// Converts a typed deletion effect into the copy pipeline's state hint.
///
/// # Parameters
///
/// * `error` - Deletion failure whose recorded effect determines the hint.
pub(super) fn deletion_failure_hint(error: &LocalFileError) -> Option<LocalCopyFailureState> {
    match error.effect_state() {
        Some(LocalFileEffectState::Unchanged) | None => None,
        Some(LocalFileEffectState::PartiallyApplied | LocalFileEffectState::Applied) => {
            Some(LocalCopyFailureState::PartiallyPublished)
        }
        Some(LocalFileEffectState::Indeterminate) => Some(LocalCopyFailureState::Indeterminate),
    }
}

/// Retains a successful conflicting-entry removal through a later error.
///
/// # Parameters
///
/// * `error` - Later copy failure to preserve or update.
/// * `removed` - Whether removal of the conflicting destination entry
///   succeeded.
pub(super) fn mark_partial_after_removal(mut error: Error, removed: bool) -> Error {
    if removed && error.failure_state() != Some(LocalCopyFailureState::Published) {
        error = error.with_failure_state(LocalCopyFailureState::PartiallyPublished);
    }
    error
}

/// Copies one rooted entry beneath the same opened root.
///
/// # Parameters
///
/// * `root` - Open root authorizing both source and destination.
/// * `source` - Existing source entry.
/// * `destination` - Destination entry beneath the same root.
/// * `options` - Explicit copy policies.
/// * `durability` - Synchronization requirement for staged regular files.
///
/// # Returns
///
/// Exact statistics accumulated by the completed copy.
///
/// # Errors
///
/// Returns a structured copy error when the source is unsupported, the
/// destination conflicts with the selected policies, traversal fails, or a
/// staged file cannot be installed.
pub(super) fn copy(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: Options,
    durability: LocalDurabilityRequirement,
) -> Result<Statistics, Error> {
    if source == destination {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            io::Error::new(
                ErrorKind::InvalidInput,
                "rooted copy source and destination must differ",
            ),
        ));
    }

    let mut budget = CopyBudget::new(options);
    if let Err(source_error) = budget.check_deadline() {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            source_error,
        ));
    }
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("rooted-copy-target-becomes-directory") {
        root.remove_file(destination).map_err(|source_error| {
            error(
                Stage::PrepareDestination,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
        root.create_dir(destination).map_err(|source_error| {
            error(
                Stage::PrepareDestination,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
    }
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("rooted-copy-source-becomes-symlink") {
        root.remove_file(source).map_err(|source_error| {
            error(
                Stage::InspectSource,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
        root.create_symlink_for_copy(std::path::Path::new("missing-source-target"), source, false)
            .map_err(|source_error| {
                let (_, source_error, _) = source_error.into_parts();
                error(
                    Stage::InspectSource,
                    source,
                    destination,
                    Statistics::default(),
                    source_error,
                )
            })?;
    }
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("rooted-copy-source-becomes-directory") {
        root.remove_file(source).map_err(|source_error| {
            error(
                Stage::InspectSource,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
        root.create_dir(source).map_err(|source_error| {
            error(
                Stage::InspectSource,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
    }
    let source_metadata = match root.symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(source_error) => {
            return Err(error(
                Stage::InspectSource,
                source,
                destination,
                Statistics::default(),
                source_error,
            ));
        }
    };
    let source_kind = match source_metadata.kind() {
        EntryKind::File => crate::LocalFileKind::File,
        EntryKind::Directory => crate::LocalFileKind::Directory,
        EntryKind::Symlink => crate::LocalFileKind::Symlink,
        #[cfg(unix)]
        EntryKind::Fifo => crate::LocalFileKind::Fifo,
        #[cfg(unix)]
        EntryKind::Socket => crate::LocalFileKind::Socket,
        #[cfg(unix)]
        EntryKind::BlockDevice => crate::LocalFileKind::BlockDevice,
        #[cfg(unix)]
        EntryKind::CharDevice => crate::LocalFileKind::CharDevice,
        EntryKind::Other => crate::LocalFileKind::Other,
    };
    crate::local::validate_copy_source_kind(source_kind, options.source_mode()).map_err(|kind| {
        error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            io::Error::new(
                ErrorKind::InvalidInput,
                format!("copy source mode rejected actual source kind: {kind:?}"),
            ),
        )
        .with_local_kind(kind)
    })?;
    if crate::local::copy_source_guarantee_unavailable(source_kind, options.atomicity(), durability) {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            io::Error::new(
                ErrorKind::Unsupported,
                "actual copy source cannot meet required guarantees",
            ),
        )
        .with_local_kind(crate::LocalFileErrorKind::RequirementNotMet));
    }
    let result = match source_metadata.kind() {
        EntryKind::File => copy_file(
            root,
            source,
            destination,
            &options,
            durability,
            Statistics::default(),
            &mut budget,
        ),
        EntryKind::Directory => {
            if destination.as_path().starts_with(source.as_path()) {
                return Err(error(
                    Stage::InspectSource,
                    source,
                    destination,
                    Statistics::default(),
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        "rooted tree destination must not be inside the source",
                    ),
                ));
            }
            copy_tree(
                root,
                source,
                destination,
                source_metadata,
                &options,
                durability,
                &mut budget,
            )
        }
        EntryKind::Symlink => copy_symlink(root, source, destination, &options, Statistics::default(), &mut budget),
        EntryKind::Other => Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            unsupported_source_error(),
        )),
        #[cfg(unix)]
        EntryKind::Fifo | EntryKind::Socket | EntryKind::BlockDevice | EntryKind::CharDevice => Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            unsupported_source_error(),
        )),
    };
    result
        .map(|mut stats| {
            stats.source_kind = Some(source_kind);
            stats
        })
        .map_err(|error| error.with_source_kind(source_kind))
}
