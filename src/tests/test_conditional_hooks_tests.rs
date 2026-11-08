// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use crate::test_support::TestConditionalHooks;
use crate::test_support::TestConditionalPoint;

#[test]
fn test_hook_parts_preserve_point_and_run_the_action_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let action_calls = Arc::clone(&calls);
    let hooks = TestConditionalHooks::once(TestConditionalPoint::AfterPublish, move || {
        action_calls.fetch_add(1, Ordering::SeqCst);
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
    assert_eq!(1, calls.load(Ordering::SeqCst));
    assert!(action.lock().expect("hook lock should be available").is_none());
}
