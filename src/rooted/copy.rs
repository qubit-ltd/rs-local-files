// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative file and directory copying.
// qubit-style: allow source-test-pair
mod destination;
mod file;
mod symlink;
mod tree;

use std::io;
use std::io::ErrorKind;

use destination::error;
use destination::unsupported_source_error;
use file::copy_file;
use symlink::copy_symlink;
use tree::copy_tree;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use crate::LocalDurabilityRequirement;
use crate::local::CopyBudget;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;

/// Copies one rooted entry beneath the same opened root.
///
/// # Parameters
///
/// * `root` - Open root authorizing both source and destination.
/// * `source` - Existing source entry.
/// * `destination` - Destination entry beneath the same root.
/// * `options` - Explicit copy policies.
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
    match source_metadata.kind() {
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
    }
}
