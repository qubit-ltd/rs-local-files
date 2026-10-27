// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Requested privacy when creating a temporary resource.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalPrivacyRequirement {
    /// Use the platform's existing temporary-resource defaults.
    #[default]
    PlatformDefault,
    /// Create the resource with access restricted to the current user.
    Required,
}
