// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use crate::LocalResult;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;

#[derive(Debug)]
struct OverflowRecorder {
    calls: Arc<AtomicUsize>,
}

impl ReaderPathBinding for OverflowRecorder {
    fn verification_deadline(&self) -> Option<Duration> {
        Some(Duration::MAX)
    }

    fn verify_until(&self, _: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(LocalSourceVerification::Unchanged)
    }
}

#[test]
fn test_path_binding_deadline_overflow_rejects_verification_before_calling_binding() {
    assert!(Instant::now().checked_add(Duration::MAX).is_none());
    let calls = Arc::new(AtomicUsize::new(0));
    let binding = OverflowRecorder { calls: calls.clone() };

    let error = binding.verify().expect_err("unrepresentable deadline is invalid");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
