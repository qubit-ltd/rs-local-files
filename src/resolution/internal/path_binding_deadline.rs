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

use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;

/// Converts a configured path-binding duration without silently dropping
/// overflow.
///
/// Returns `Ok(None)` when no duration is configured. An unrepresentable
/// duration returns `InvalidOptions` and retains the supplied operation and
/// optional path in the error context.
pub(crate) fn checked_path_binding_deadline(
    started_at: Instant,
    duration: Option<Duration>,
    operation: LocalFileOperation,
    path: Option<&Path>,
) -> LocalResult<Option<Instant>> {
    duration
        .map(|duration| {
            started_at.checked_add(duration).ok_or_else(|| {
                let error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
                    .with_reason("path binding deadline exceeds the monotonic clock range");
                match path {
                    Some(path) => error.with_path(path.to_path_buf()),
                    None => error,
                }
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::time::Instant;

    use super::checked_path_binding_deadline;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileOperation;

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
        let path = std::path::Path::new("item");
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
}
