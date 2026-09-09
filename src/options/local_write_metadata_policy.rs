// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Metadata policy for staged replacement of an existing local file.

/// Selects whether replacement requests preservation of old destination
/// metadata.
///
/// This policy only affects replacement. Creating a new entry publishes its
/// staging metadata, and appending leaves metadata changes to native writes.
/// Neither policy relaxes destination identity or no-replacement checks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalWriteMetadataPolicy {
    /// Preserve metadata using the existing backend protocol, failing rather
    /// than silently dropping unsupported attributes. Unix copies owner,
    /// permissions and supported extended metadata; Windows Host uses native
    /// replacement merging, while Windows Rooted preserves portable
    /// permissions.
    #[default]
    PreserveExisting,
    /// Publish staging metadata without requesting old destination merging.
    ///
    /// This can change access controls and ownership. Native inheritance rules
    /// still apply; the library does not read old file content for
    /// preservation.
    UseStaging,
}
