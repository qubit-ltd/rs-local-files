// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::ConditionalBackend;
use super::ConditionalEntry;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::recovery::LocalRecoveryFile;

/// Removes verified recovery entries in the protocol's required order.
///
/// Displaced and rollback entries are processed before the independently
/// verified candidate copy. Each entry is identity-checked immediately before
/// removal. A failed checkpoint, identity check, or removal records a cleanup
/// error and transfers that entry into the returned recovery list; it is never
/// treated as successfully removed.
pub(super) fn cleanup_entries<B: ConditionalBackend>(
    backend: &mut B,
    mut entries: Vec<ConditionalEntry>,
) -> (Vec<LocalFileError>, Vec<LocalRecoveryFile>) {
    entries.sort_by_key(|entry| entry.role() == crate::recovery::LocalRecoveryFileRole::CandidateCopy);
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
