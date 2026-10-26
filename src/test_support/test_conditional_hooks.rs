// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use super::TestConditionalPoint;

type TestAction = Box<dyn FnOnce() -> io::Result<()> + Send>;

/// One-shot action attached to one conditional publication boundary.
pub struct TestConditionalHooks {
    point: TestConditionalPoint,
    action: Arc<Mutex<Option<TestAction>>>,
}

impl std::fmt::Debug for TestConditionalHooks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TestConditionalHooks")
            .field("point", &self.point)
            .finish_non_exhaustive()
    }
}

impl TestConditionalHooks {
    /// Runs `action` once when `point` is reached.
    pub fn once<F>(point: TestConditionalPoint, action: F) -> Self
    where
        F: FnOnce() -> io::Result<()> + Send + 'static,
    {
        Self {
            point,
            action: Arc::new(Mutex::new(Some(Box::new(action)))),
        }
    }

    pub(crate) fn into_parts(self) -> (TestConditionalPoint, Arc<Mutex<Option<TestAction>>>) {
        (self.point, self.action)
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::TestConditionalHooks;
    use crate::test_support::TestConditionalPoint;

    #[test]
    fn test_debug_shows_hook_point_without_exposing_action() {
        let hooks = TestConditionalHooks::once(TestConditionalPoint::BeforePublish, || Ok::<(), io::Error>(()));

        let debug = format!("{hooks:?}");
        assert!(debug.contains("BeforePublish"));
        assert!(!debug.contains("action"));
    }
}
