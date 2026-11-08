// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows conditional publication, receipt classification, and backend
//! orchestration.

mod install;
mod namespace;
mod native_receipt_error;
mod private_entry_guard;
mod private_entry_setup_error;
mod recovery;
mod windows_conditional_backend;

pub(super) use windows_conditional_backend::WindowsConditionalBackend;

#[cfg(test)]
mod tests;

use std::ffi::OsStr;
use std::fs::File;
use std::io;

use namespace::FileIdentity;
use namespace::Namespace;
use namespace::file_identity;
use native_receipt_error::NativeReceiptError;
use private_entry_guard::PrivateEntryGuard;

use super::ConditionalEntry;
use crate::outcome::LocalConditionalPublicationState;
use crate::recovery::LocalRecoveryFileRole;

/// Returns the publication method used when replacing a destination with a
/// backup.
#[cfg(feature = "digest")]
fn replace_with_backup_publication_method() -> crate::outcome::LocalWritePublicationMethod {
    crate::outcome::LocalWritePublicationMethod::AtomicReplaceWithBackup
}

/// Returns the publication method used when replacing a destination without
/// digest support.
#[cfg(not(feature = "digest"))]
fn replace_with_backup_publication_method() -> crate::outcome::LocalWritePublicationMethod {
    crate::outcome::LocalWritePublicationMethod::AtomicRename
}

/// Opens an optional entry, treating a missing entry as absent.
///
/// # Type Parameters
///
/// * `T` is the type of the opened entry.
///
/// # Parameters
///
/// * `open` opens the entry when it exists.
fn observe_optional_entry<T>(open: impl FnOnce() -> io::Result<T>) -> io::Result<Option<T>> {
    match open() {
        Ok(entry) => Ok(Some(entry)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Separates an optional observed entry from any error produced while observing
/// it.
///
/// # Type Parameters
///
/// * `T` is the type of the observed entry.
///
/// # Parameters
///
/// * `result` is the result of observing the optional entry.
fn observe_with_error<T>(result: io::Result<Option<T>>) -> (Option<T>, Option<io::Error>) {
    match result {
        Ok(entry) => (entry, None),
        Err(error) => (None, Some(error)),
    }
}

/// Combines native operation and observation failures while preserving the
/// native error kind.
///
/// # Parameters
///
/// * `native_error` is the error returned by the native operation, if any.
/// * `observation_error` is the error returned while observing the operation,
///   if any.
fn combine_observation_error(
    native_error: Option<io::Error>,
    observation_error: Option<io::Error>,
) -> Option<io::Error> {
    match (native_error, observation_error) {
        (Some(native), Some(observation)) => {
            let kind = native.kind();
            Some(io::Error::new(kind, NativeReceiptError { native, observation }))
        }
        (Some(error), None) | (None, Some(error)) => Some(error),
        (None, None) => None,
    }
}

/// Classifies what the post-operation namespace observation proves.
///
/// An observation error makes the result indeterminate; otherwise an installed
/// candidate proves publication, a remaining candidate without a backup proves
/// no publication, and every unresolved shape remains indeterminate.
///
/// # Parameters
///
/// * `installed_is_candidate` indicates whether the installed entry has the
///   candidate identity.
/// * `candidate_present` reports whether the staged candidate name remains
///   present.
/// * `backup_present` reports whether a displaced backup entry is present.
/// * `observation_failed` reports whether any required namespace observation
///   failed.
///
/// # Returns
///
/// The publication state justified by the observed namespace entries.
pub(super) fn receipt_publication(
    installed_is_candidate: Option<bool>,
    candidate_present: bool,
    backup_present: bool,
    observation_failed: bool,
) -> LocalConditionalPublicationState {
    if observation_failed {
        LocalConditionalPublicationState::Indeterminate
    } else if installed_is_candidate == Some(true) {
        LocalConditionalPublicationState::Published
    } else if candidate_present && !backup_present {
        LocalConditionalPublicationState::NotPublished
    } else {
        LocalConditionalPublicationState::Indeterminate
    }
}

/// Reopens a staged candidate and verifies its identity while retaining cleanup
/// ownership.
///
/// # Parameters
///
/// * `namespace` supplies the namespace used to reopen and clean up the
///   candidate.
/// * `candidate_name` identifies the staged entry within that namespace.
/// * `candidate_file` is the open handle whose identity must be verified;
///   cleanup ownership remains with this function until verification succeeds.
///
/// # Returns
///
/// The verified entry and its stable file identity.
///
/// # Errors
///
/// Returns an I/O error if identity inspection or reopening fails; cleanup is
/// attempted before the error is returned.
pub(in crate::local::internal::conditional::windows) fn open_verified_candidate_entry(
    namespace: &Namespace,
    candidate_name: &OsStr,
    candidate_file: &mut File,
) -> io::Result<(ConditionalEntry, FileIdentity)> {
    let mut guard = match PrivateEntryGuard::new(namespace, candidate_name, candidate_file) {
        Ok(guard) => guard,
        Err((primary, mut guard)) => return Err(guard.finish_with(primary)),
    };
    let candidate_identity = match file_identity(guard.file_mut()) {
        Ok(identity) => identity,
        Err(primary) => return Err(guard.finish_with(primary)),
    };
    let candidate_entry = match open_verified_private_entry(
        namespace,
        LocalRecoveryFileRole::StagedCandidate,
        candidate_name,
        guard.file_mut(),
        candidate_identity,
    ) {
        Ok(entry) => entry,
        Err(primary) => return Err(guard.finish_with(primary)),
    };
    guard.disarm();
    Ok((candidate_entry, candidate_identity))
}

/// Reopens a private entry and binds it to the expected file identity.
///
/// # Parameters
///
/// * `namespace` supplies the namespace used to reopen and bind the entry.
/// * `role` identifies the recovery-file role assigned to the entry.
/// * `name` identifies the entry within the namespace.
/// * `file` is the open handle to which the reopened entry must be bound.
/// * `expected_identity` is the identity the reopened entry must match.
///
/// # Returns
///
/// The reopened entry bound to the supplied file handle.
///
/// # Errors
///
/// Returns an I/O error if the entry cannot be opened or its identity differs.
pub(in crate::local::internal::conditional::windows) fn open_verified_private_entry(
    namespace: &Namespace,
    role: LocalRecoveryFileRole,
    name: &OsStr,
    file: &File,
    expected_identity: FileIdentity,
) -> io::Result<ConditionalEntry> {
    let observed = namespace.entry(role, name.to_os_string())?;
    let reader = observed.open_reader().map_err(io::Error::other)?;
    if file_identity(reader.as_file())? != expected_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "conditional private entry name changed identity while reopening",
        ));
    }
    namespace.entry_bound_to(role, name.to_os_string(), file)
}
