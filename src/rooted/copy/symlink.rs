// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Symbolic-link copy publication.

use std::io;
use std::io::ErrorKind;

use super::CopyBudget;
use super::EntryKind;
use super::Error;
use super::Options;
use super::Path;
use super::Root;
use super::Stage;
use super::Statistics;
use super::destination::checked_add;
use super::destination::error;
use super::destination::optional_metadata;
use crate::local::CopyDestinationAction;
use crate::local::RootedSymlinkCreateFailureState;
use crate::local::decide_copy_destination;

/// Copies a symbolic-link entry without dereferencing its stored target.
///
/// # Errors
///
/// Returns a structured copy failure when destination preparation, source-link
/// inspection, or final link publication fails.
pub(super) fn copy_symlink(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: &Options,
    mut statistics: Statistics,
    budget: &mut CopyBudget,
) -> Result<Statistics, Error> {
    let destination_metadata = match optional_metadata(root, destination) {
        Ok(metadata) => metadata,
        Err(source_error) => {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                source_error,
            ));
        }
    };
    let destination_is_directory = match destination_metadata.as_ref() {
        Some(metadata) => metadata.kind() == EntryKind::Directory,
        None => false,
    };
    let action = decide_copy_destination(
        false,
        destination_metadata.as_ref().map(|_| destination_is_directory),
        options.conflict_policy(),
        options.type_conflict_policy(),
    );
    let action = match action {
        Some(action) => action,
        None => {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                io::Error::from(ErrorKind::AlreadyExists),
            ));
        }
    };
    let replaced_existing = action == CopyDestinationAction::Replace;
    match action {
        CopyDestinationAction::Skip => {
            statistics.skipped = checked_add(statistics.skipped, 1, source, destination, statistics)?;
            return Ok(statistics);
        }
        CopyDestinationAction::Replace => {}
        CopyDestinationAction::Create => {}
        CopyDestinationAction::Merge => {
            unreachable!("a symbolic link destination cannot require merge")
        }
    }
    let link_target = match root.read_link(source) {
        Ok(target) => target,
        Err(source_error) => {
            return Err(error(
                Stage::InspectSourceEntry,
                source,
                destination,
                statistics,
                source_error,
            ));
        }
    };
    if let Err(source_error) = budget.check_deadline() {
        return Err(error(
            Stage::InspectSourceEntry,
            source,
            destination,
            statistics,
            source_error,
        ));
    }
    #[cfg(windows)]
    let targets_directory = root
        .symlink_targets_directory(source)
        .map_err(|source_error| error(Stage::InspectSourceEntry, source, destination, statistics, source_error))?;
    #[cfg(not(windows))]
    let targets_directory = false;
    if replaced_existing {
        let removal = if destination_is_directory {
            root.remove_tree(destination)
        } else {
            root.remove_file(destination)
        };
        if let Err(source_error) = removal {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                source_error,
            ));
        }
    }
    root.create_symlink_for_copy(&link_target, destination, targets_directory)
        .map_err(|failure| {
            let (state, primary, cleanup) = failure.into_parts();
            let stage = publication_failure_stage(state, replaced_existing);
            let failure = error(stage, source, destination, statistics, primary);
            match cleanup {
                Some(cleanup) => failure.with_cleanup_error(cleanup),
                None => failure,
            }
        })?;
    statistics.files = checked_add(statistics.files, 1, source, destination, statistics)?;
    if destination_metadata.is_some() {
        statistics.overwritten = checked_add(statistics.overwritten, 1, source, destination, statistics)?;
    }
    statistics.non_atomic_publication = true;
    statistics.files_durable = false;
    Ok(statistics)
}

/// Maps native publication facts plus prior destination removal into the
/// strongest end-to-end copy state.
fn publication_failure_stage(state: RootedSymlinkCreateFailureState, replaced_existing: bool) -> Stage {
    match state {
        RootedSymlinkCreateFailureState::Unchanged if replaced_existing => Stage::PublishSymlinkPartially,
        RootedSymlinkCreateFailureState::Unchanged => Stage::PublishSymlinkUnchanged,
        #[cfg(windows)]
        RootedSymlinkCreateFailureState::PartiallyPublished => Stage::PublishSymlinkPartially,
        #[cfg(windows)]
        RootedSymlinkCreateFailureState::Indeterminate => Stage::PublishSymlinkIndeterminate,
    }
}

#[cfg(test)]
mod tests {
    use super::RootedSymlinkCreateFailureState;
    use super::Stage;
    use super::publication_failure_stage;

    /// Verifies rollback before any destination change preserves unchanged.
    #[test]
    fn test_publication_failure_without_replacement_is_unchanged() {
        assert_eq!(
            Stage::PublishSymlinkUnchanged,
            publication_failure_stage(RootedSymlinkCreateFailureState::Unchanged, false),
        );
    }

    /// Verifies deleting an old destination prevents an unchanged result even
    /// when publication rolls back its new placeholder.
    #[test]
    fn test_publication_failure_after_replacement_is_partial() {
        assert_eq!(
            Stage::PublishSymlinkPartially,
            publication_failure_stage(RootedSymlinkCreateFailureState::Unchanged, true),
        );
    }
}
