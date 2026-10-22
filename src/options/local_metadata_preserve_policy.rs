// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by copy integration tests.

/// Metadata preservation requested during native copy or persistence.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalCopyOptions;
/// use qubit_local_files::options::LocalMetadataPreservePolicy;
///
/// let options = LocalCopyOptions::new()
///     .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions);
/// assert_eq!(
///     options.preserve_metadata(),
///     LocalMetadataPreservePolicy::Permissions,
/// );
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalMetadataPreservePolicy {
    /// Do not copy source metadata.
    #[default]
    None,
    /// Preserve portable native permission metadata.
    Permissions,
}
