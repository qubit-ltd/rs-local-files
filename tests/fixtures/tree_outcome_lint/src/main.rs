// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![deny(unused_must_use)]

use qubit_local_files::directory::LocalTreeValidationOptions;
use qubit_local_files::outcome::LocalTreePublishOutcome;

fn relay(outcome: LocalTreePublishOutcome) -> LocalTreePublishOutcome {
    outcome
}

#[cfg(feature = "drop-result")]
fn discard(outcome: LocalTreePublishOutcome) {
    relay(outcome);
}

#[cfg(feature = "drop-validation-options")]
fn discard_validation_options() {
    LocalTreeValidationOptions::new().with_reject_symlinks();
}

#[cfg(not(feature = "drop-validation-options"))]
fn consume_validation_options() {
    let _options = LocalTreeValidationOptions::new().with_reject_symlinks();
}

#[cfg(not(feature = "drop-result"))]
fn inspect(outcome: LocalTreePublishOutcome) {
    let _ = (outcome.durable(), outcome.file_synced(), outcome.access_state());
}

fn main() {
    #[cfg(feature = "drop-validation-options")]
    discard_validation_options();
    #[cfg(not(feature = "drop-validation-options"))]
    consume_validation_options();
}
