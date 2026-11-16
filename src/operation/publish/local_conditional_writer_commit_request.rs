// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalWriteOptions;

#[cfg(feature = "digest")]
/// Owned request for a digest-preconditioned writer commit.
#[derive(Debug)]
pub struct LocalConditionalWriterCommitRequest<D: digest::Digest> {
    /// Destination interpreted through the facade authority.
    pub path: PathBuf,
    /// Digest/absence condition checked by the native conditional state
    /// machine.
    pub precondition: crate::options::LocalWritePrecondition<digest::Output<D>>,
    /// Publication policy.
    pub options: LocalWriteOptions,
    /// Owned candidate bytes.
    pub bytes: Vec<u8>,
}
