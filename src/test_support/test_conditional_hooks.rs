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

    #[test]
    fn test_hook_parts_preserve_point_and_run_the_action_once() {
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let action_calls = std::sync::Arc::clone(&calls);
        let hooks = TestConditionalHooks::once(TestConditionalPoint::AfterPublish, move || {
            action_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok::<(), io::Error>(())
        });

        let (point, action) = hooks.into_parts();
        assert_eq!(TestConditionalPoint::AfterPublish, point);
        let run = action
            .lock()
            .expect("hook lock should be available")
            .take()
            .expect("action should exist");
        run().expect("hook action should succeed");
        assert_eq!(1, calls.load(std::sync::atomic::Ordering::SeqCst));
        assert!(action.lock().expect("hook lock should be available").is_none());
    }
}
