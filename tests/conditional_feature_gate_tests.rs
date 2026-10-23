// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use qubit_local_files::LocalConditionalWriter;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalConditionalCommitError;
use qubit_local_files::error::LocalConditionalFailureReason;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalConditionalPublicationState;
use qubit_local_files::outcome::LocalConditionalWriteOutcome;
use qubit_local_files::recovery::LocalRecoveryFile;
use qubit_local_files::recovery::LocalRecoveryFileRole;

#[test]
fn conditional_public_types_are_available_from_their_stable_modules() {
    let _writer: Option<LocalConditionalWriter<sha2::Sha256>> = None;
    let _filesystem_method = LocalFileSystem::open_conditional_writer::<sha2::Sha256>;
    let _precondition: Option<LocalWritePrecondition<sha2::digest::Output<sha2::Sha256>>> = None;
    let _error: Option<LocalConditionalCommitError> = None;
    let _reason: Option<LocalConditionalFailureReason> = None;
    let _publication: Option<LocalConditionalPublicationState> = None;
    let _outcome: Option<LocalConditionalWriteOutcome<sha2::Sha256>> = None;
    let _recovery: Option<LocalRecoveryFile> = None;
    let _role: Option<LocalRecoveryFileRole> = None;
}
