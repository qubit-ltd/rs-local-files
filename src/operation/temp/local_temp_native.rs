// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalTempCleanupAnchor;

/// Native owner currently attached to a registered temporary-file resource.
///
/// Variants encode lifecycle state so serialized commands can reject
/// transitions that are invalid for the current kind of native object.
pub(crate) enum LocalTempNative {
    /// Newly created, writable temporary file.
    File(
        /// Writable native file owner.
        crate::LocalTempFile,
    ),
    /// Sealed file that can be exported through a read-only handle.
    Sealed(
        /// Read-only native file owner after sealing.
        crate::LocalSealedTempFile,
    ),
    /// Temporary directory managed by the same registered resource.
    Directory(
        /// Native directory owner tracked as a managed resource.
        crate::LocalTempDirectory,
    ),
    /// Recovery owner retained after cleanup or lifecycle failure.
    Recovery(
        /// Cleanup recovery owner retained after a lifecycle failure.
        crate::LocalTempRecovery,
    ),
    /// A consuming command panicked after taking its owner. The cleanup-only
    /// clone retains the same Host/Rooted authority and identity checks.
    Unconfirmed(
        /// Cleanup authority retained after a consuming command panicked.
        LocalTempCleanupAnchor,
    ),
}
