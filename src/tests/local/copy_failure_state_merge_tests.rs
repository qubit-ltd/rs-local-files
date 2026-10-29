// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Independent copy destination-effect merge tests.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalAtomicDestinationState;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::outcome::LocalCopyFailure;
use crate::outcome::LocalCopyFailureState;
use crate::outcome::merge_copy_failure_state;

#[test]
fn test_copy_failure_state_merges_independent_destination_effects() {
    let cases = [
        (LocalCopyFailureState::Unchanged, None, LocalCopyFailureState::Unchanged),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Unchanged,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Unchanged,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            None,
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::PartiallyPublished,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (LocalCopyFailureState::Published, None, LocalCopyFailureState::Published),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Published,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            None,
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Unchanged),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Published),
            LocalCopyFailureState::Published,
        ),
        (
            LocalCopyFailureState::Indeterminate,
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
    ];

    for (inferred, hint, expected) in cases {
        assert_eq!(expected, merge_copy_failure_state(inferred, hint));
    }
}

#[test]
fn test_copy_atomic_preinstall_fact_preserves_prior_effects() {
    let cases = [
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats::default(),
            None,
            LocalCopyFailureState::Unchanged,
        ),
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats {
                files: 1,
                ..LocalCopyDirStats::default()
            },
            None,
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats {
                directories: 1,
                ..LocalCopyDirStats::default()
            },
            None,
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats {
                overwritten: 1,
                ..LocalCopyDirStats::default()
            },
            None,
            LocalCopyFailureState::Unchanged,
        ),
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats::default(),
            Some(LocalCopyFailureState::PartiallyPublished),
            LocalCopyFailureState::PartiallyPublished,
        ),
        (
            Some(LocalAtomicDestinationState::Unchanged),
            LocalCopyDirStats::default(),
            Some(LocalCopyFailureState::Indeterminate),
            LocalCopyFailureState::Indeterminate,
        ),
        (
            None,
            LocalCopyDirStats::default(),
            None,
            LocalCopyFailureState::Indeterminate,
        ),
        (
            Some(LocalAtomicDestinationState::Indeterminate),
            LocalCopyDirStats::default(),
            None,
            LocalCopyFailureState::Indeterminate,
        ),
        (
            Some(LocalAtomicDestinationState::Missing),
            LocalCopyDirStats::default(),
            None,
            LocalCopyFailureState::Indeterminate,
        ),
    ];

    for (atomic_state, stats, failure_hint, expected_state) in cases {
        let mut error = LocalCopyDirError::new(
            LocalCopyDirStage::CommitFile,
            PathBuf::from("failed-source"),
            PathBuf::from("failed-target"),
            stats,
            io::Error::other("commit failed"),
        );
        if let Some(atomic_state) = atomic_state {
            error = error.with_atomic_destination_state(atomic_state);
        }
        if let Some(failure_hint) = failure_hint {
            error = error.with_failure_state(failure_hint);
        }

        let failure =
            LocalCopyFailure::from_copy_dir_error(Path::new("request-source"), Path::new("request-target"), error);

        assert_eq!(expected_state, failure.state());
        assert_eq!(Some(Path::new("failed-source")), failure.failed_source_path());
        assert_eq!(Some(Path::new("failed-target")), failure.failed_target_path());
        assert_eq!(crate::error::LocalFileErrorKind::Io, failure.error().kind());
    }
}
