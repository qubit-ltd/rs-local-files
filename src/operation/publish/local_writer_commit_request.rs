// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::options::LocalWriteOptions;

/// Owned data and policy for one atomic writer commit.
#[derive(Debug)]
pub struct LocalWriterCommitRequest {
    /// Destination interpreted through the facade's captured authority.
    pub path: PathBuf,
    /// Writer policy fixed for this command.
    pub options: LocalWriteOptions,
    /// Bytes owned by the operation before submission.
    pub bytes: Vec<u8>,
}
