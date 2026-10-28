// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Unix platform family used to interpret filesystem observations.
pub enum LocalUnixPlatform {
    /// Linux.
    Linux,
    /// Darwin (macOS).
    Darwin,
    /// Another Unix platform.
    Other,
}
