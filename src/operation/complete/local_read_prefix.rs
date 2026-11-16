// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// An at-most prefix result. `complete` is true only when EOF was observed.
#[derive(Debug, Eq, PartialEq)]
pub struct LocalReadPrefix {
    /// Bytes from the start of the opened object.
    pub bytes: Vec<u8>,
    /// Whether EOF was observed before or at the requested prefix boundary.
    pub complete: bool,
}
