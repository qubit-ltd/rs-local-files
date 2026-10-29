// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![deny(unused_must_use)]

fn relay(
    outcome: qubit_local_files::outcome::LocalTreePublishOutcome,
) -> qubit_local_files::outcome::LocalTreePublishOutcome {
    outcome
}

#[cfg(feature = "drop-result")]
fn discard(outcome: qubit_local_files::outcome::LocalTreePublishOutcome) {
    relay(outcome);
}

#[cfg(not(feature = "drop-result"))]
fn inspect(outcome: qubit_local_files::outcome::LocalTreePublishOutcome) {
    let _ = (outcome.durable(), outcome.file_synced(), outcome.access_state());
}

fn main() {}
