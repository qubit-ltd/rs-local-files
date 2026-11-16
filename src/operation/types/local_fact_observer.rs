// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::Mutex;

use super::LocalFactSnapshot;

/// Read-only observer for the bounded local fact snapshot.
#[derive(Clone)]
pub struct LocalFactObserver {
    /// Stores the snapshot associated with this value.
    snapshot: Arc<Mutex<LocalFactSnapshot>>,
    /// Stores the  record permit associated with this value.
    _record_permit: Option<Arc<dyn Send + Sync>>,
}

impl std::fmt::Debug for LocalFactObserver {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalFactObserver")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl LocalFactObserver {
    /// Returns the latest fixed-size snapshot without retaining a resource
    /// reference.
    pub fn snapshot(&self) -> LocalFactSnapshot {
        *self.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(LocalFactSnapshot::default())),
            _record_permit: None,
        }
    }
    /// Creates an observer whose lifetime retains a bounded fact-record slot.
    ///
    /// # Parameters
    /// - `permit` — reservation returned when the observer is dropped.
    #[cfg(feature = "async")]
    pub(crate) fn with_permit(permit: Arc<dyn Send + Sync>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(LocalFactSnapshot::default())),
            _record_permit: Some(permit),
        }
    }

    /// Runs the update behavior while preserving the operation lifecycle
    /// invariants.

    /// # Parameters
    /// - `snapshot` — supplies data or policy for this operation.
    #[allow(dead_code)]
    pub(crate) fn update(&self, snapshot: LocalFactSnapshot) {
        *self.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot;
    }
}
