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

#[cfg(test)]
mod tests {
    use super::LocalFactObserver;
    use super::LocalFactSnapshot;

    #[test]
    fn test_observer_clone_reads_the_updated_fixed_size_snapshot() {
        let observer = LocalFactObserver::new();
        let clone = observer.clone();
        assert_eq!(observer.snapshot(), LocalFactSnapshot::default());

        let snapshot = LocalFactSnapshot {
            modified_count: 2,
            modified_bytes: 5,
            range_unknown: true,
            ..LocalFactSnapshot::default()
        };
        observer.update(snapshot);

        assert_eq!(clone.snapshot(), snapshot);
        let debug = format!("{clone:?}");
        assert!(debug.contains("modified_count: 2"));
        assert!(debug.contains("modified_bytes: 5"));
    }

    #[test]
    fn test_snapshot_recovers_the_latest_value_after_mutex_poisoning() {
        let observer = LocalFactObserver::new();
        let expected = LocalFactSnapshot {
            modified_count: 3,
            ..LocalFactSnapshot::default()
        };
        observer.update(expected);

        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observer.snapshot.lock().unwrap();
            panic!("poison observer snapshot mutex");
        }));

        assert_eq!(observer.snapshot(), expected);
    }

    #[test]
    fn test_update_recovers_and_replaces_value_after_mutex_poisoning() {
        let observer = LocalFactObserver::new();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = observer.snapshot.lock().unwrap();
            panic!("poison observer snapshot mutex");
        }));

        let expected = LocalFactSnapshot {
            modified_bytes: 11,
            range_unknown: true,
            ..LocalFactSnapshot::default()
        };
        observer.update(expected);

        assert_eq!(observer.snapshot(), expected);
    }
}
