// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalWriteOptions;

/// Owned request for a digest-preconditioned writer commit.
///
/// # Type Parameters
/// * `D` — digest algorithm whose output is checked before publication.
#[cfg(feature = "digest")]
#[derive(Debug)]
pub struct LocalConditionalWriterCommitRequest<D: digest::Digest> {
    /// Destination interpreted through the facade authority.
    /// Destination resolved through the facade authority.
    pub path: PathBuf,
    /// Digest/absence condition checked by the native conditional state
    /// machine.
    /// Digest or absence condition checked immediately before publication.
    pub precondition: crate::options::LocalWritePrecondition<digest::Output<D>>,
    /// Publication policy.
    /// Durability and access policy for the committed file.
    pub options: LocalWriteOptions,
    /// Owned candidate bytes.
    /// Candidate contents owned by this request.
    pub bytes: Vec<u8>,
}
