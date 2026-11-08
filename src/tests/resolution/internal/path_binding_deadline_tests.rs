// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::resolution::internal::checked_path_binding_deadline;

#[test]
fn test_checked_path_binding_deadline_values_and_overflow() {
    let started_at = Instant::now();
    assert_eq!(
        checked_path_binding_deadline(started_at, None, LocalFileOperation::Metadata, None)
            .expect("no limit is accepted"),
        None
    );
    assert_eq!(
        checked_path_binding_deadline(started_at, Some(Duration::ZERO), LocalFileOperation::Metadata, None,)
            .expect("zero duration is representable"),
        Some(started_at)
    );
    assert!(
        checked_path_binding_deadline(
            started_at,
            Some(Duration::from_secs(1)),
            LocalFileOperation::Metadata,
            None,
        )
        .expect("finite duration is representable")
        .is_some()
    );
    assert!(started_at.checked_add(Duration::MAX).is_none());
    let path = Path::new("item");
    let error = checked_path_binding_deadline(
        started_at,
        Some(Duration::MAX),
        LocalFileOperation::Metadata,
        Some(path),
    )
    .expect_err("overflow is rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(path));
    assert_eq!(
        error.reason(),
        Some("path binding deadline exceeds the monotonic clock range")
    );
}
