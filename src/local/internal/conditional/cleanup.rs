// =============================================================================

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::recovery::LocalRecoveryFile;

use super::ConditionalBackend;
use super::ConditionalEntry;

pub(super) fn cleanup_entries<B: ConditionalBackend>(
    backend: &mut B,
    mut entries: Vec<ConditionalEntry>,
) -> (Vec<LocalFileError>, Vec<LocalRecoveryFile>) {
    entries
        .sort_by_key(|entry| entry.role() == crate::recovery::LocalRecoveryFileRole::CandidateCopy);
    let mut errors = Vec::new();
    let mut retained = Vec::new();
    for entry in entries {
        let checkpoint = if entry.role() == crate::recovery::LocalRecoveryFileRole::CandidateCopy {
            super::ConditionalPoint::CleanupCopy
        } else {
            super::ConditionalPoint::CleanupOld
        };
        if let Err(source) = backend
            .checkpoint(checkpoint)
            .and_then(|()| backend.verify_entry(&entry))
            .and_then(|()| backend.remove_observed(&entry))
        {
            errors.push(LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(entry.namespace_path().to_path_buf()),
                None,
                source,
            ));
            let role = entry.role();
            retained.push(entry.into_recovery(role));
        }
    }
    (errors, retained)
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
