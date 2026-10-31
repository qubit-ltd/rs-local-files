// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by copy integration tests.

use super::LocalCopyMethod;
use super::LocalCopyStats;
use crate::LocalMetadataPreservePolicy;

/// Structured result of a native file or directory copy.
///
/// The outcome reports transfer statistics together with the method and
/// guarantees achieved by the copy pipeline.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
///
/// let directory = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let source = directory.path().join("source.txt");
/// let target = directory.path().join("copy.txt");
/// std::fs::write(&source, b"copy example")?;
/// let copy = filesystem.copy(&source, &target);
/// #[cfg(target_os = "freebsd")]
/// {
///     let failure = copy.expect_err("FreeBSD is temporarily unsupported for copy");
///     assert_eq!(
///         failure.error().kind(),
///         qubit_local_files::error::LocalFileErrorKind::Unsupported,
///     );
/// }
/// #[cfg(not(target_os = "freebsd"))]
/// {
///     let outcome = copy?;
///     assert_eq!(outcome.stats().bytes(), 12);
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalCopyOutcome {
    /// Aggregate copy statistics.
    stats: LocalCopyStats,
    /// Method used to perform the copy.
    method: LocalCopyMethod,
    /// Whether destination publication was atomic as one operation.
    atomic: bool,
    /// Whether required durability synchronization completed.
    durable: bool,
    /// Metadata preservation applied to copied entries; skipped entries are
    /// not covered by this policy.
    metadata_preservation: LocalMetadataPreservePolicy,
}

impl LocalCopyOutcome {
    /// Creates a copy outcome from verified implementation results.
    ///
    /// # Parameters
    ///
    /// - `stats`: Aggregate copy statistics.
    /// - `method`: Native copy method.
    /// - `atomic`: Whether the whole publication was atomic.
    /// - `durable`: Whether durability synchronization completed.
    /// - `metadata_preservation`: Metadata preservation applied to copied
    ///   entries.
    ///
    /// # Returns
    ///
    /// An outcome containing the supplied statistics, method, and achieved
    /// copy guarantees.
    pub(crate) const fn new(
        stats: LocalCopyStats,
        method: LocalCopyMethod,
        atomic: bool,
        durable: bool,
        metadata_preservation: LocalMetadataPreservePolicy,
    ) -> Self {
        Self {
            stats,
            method,
            atomic,
            durable,
            metadata_preservation,
        }
    }

    /// Returns aggregate copy statistics.
    #[must_use = "the aggregate copy statistics should be inspected"]
    #[inline]
    pub const fn stats(&self) -> LocalCopyStats {
        self.stats
    }

    /// Returns the method used to copy the entry.
    #[must_use = "the copy publication method should be inspected"]
    #[inline]
    pub const fn method(&self) -> LocalCopyMethod {
        self.method
    }

    /// Reports whether the entire destination publication was atomic.
    #[must_use]
    #[inline]
    pub const fn atomic(&self) -> bool {
        self.atomic
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[inline]
    pub const fn durable(&self) -> bool {
        self.durable
    }

    /// Returns metadata preservation applied to copied entries.
    ///
    /// Returns [`LocalMetadataPreservePolicy::None`] when no entries were
    /// copied. Skipped entries are not covered by the reported policy.
    #[must_use = "the applied metadata policy should be inspected"]
    #[inline]
    pub const fn metadata_preservation(&self) -> LocalMetadataPreservePolicy {
        self.metadata_preservation
    }
}
