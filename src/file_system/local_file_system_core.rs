// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared immutable state retained by filesystem clones.

use std::io;

use crate::LocalFileSystemCapabilities;
use crate::LocalFileSystemLimits;
use crate::resolution::ResolutionContract;
#[cfg(feature = "test-support")]
use crate::TestFaultPlan;
use crate::local::LocalNamespace;
use crate::test_support::TestFaultPoint;

/// Immutable state shared by all clones of a configured filesystem.
pub(crate) struct LocalFileSystemCore {
    /// Native Host or opened Rooted authority selected at construction.
    pub(crate) namespace: LocalNamespace,
    /// Immutable target capability snapshot shared by clones.
    pub(crate) capabilities: LocalFileSystemCapabilities,
    /// Authority-level path-limit observation shared by clones.
    pub(crate) limits: LocalFileSystemLimits,
    /// Effective resolution restrictions retained by this filesystem.
    pub(crate) resolution: ResolutionContract,
    /// Instance-local deterministic faults available only to tests.
    #[cfg(feature = "test-support")]
    pub(crate) test_faults: Option<TestFaultPlan>,
    /// Instance-local conditional publication checkpoints for integration
    /// tests.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    pub(crate) conditional_hooks: Option<crate::local::conditional::ConditionalHooks>,
}

impl std::fmt::Debug for LocalFileSystemCore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalFileSystemCore")
            .field("namespace", &self.namespace)
            .field("capabilities", &self.capabilities)
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl LocalFileSystemCore {
    /// Returns an injected error for one operation boundary when test support
    /// is enabled and a matching fault is configured.
    ///
    /// # Parameters
    ///
    /// - `point`: Operation boundary to inspect.
    ///
    /// # Errors
    ///
    /// Returns the configured injected I/O error when `point` matches a
    /// pending fault; otherwise returns `Ok(())`.
    pub(crate) fn fail_if_requested(&self, point: TestFaultPoint) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        {
            if let Some(error) = self.test_faults.as_ref().and_then(|plan| plan.take(point)) {
                return Err(error);
            }
        }
        let _ = point;
        Ok(())
    }
}
