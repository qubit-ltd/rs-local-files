// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// A structured stream commit error. When `retryable()` is true, the native
/// writer remains owned by the async stream handle and `commit` may be retried.
///
/// The writer's `commit` operation produces this error; callers can inspect the
/// structured cause and decide whether to retry on the same handle.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamCommitError;
///
/// fn may_retry(error: &LocalStreamCommitError) -> bool {
///     let _cause = error.commit_error();
///     error.retryable()
/// }
/// ```
#[derive(Debug)]
pub struct LocalStreamCommitError {
    /// Structured publication failure preserved from the native writer.
    error: crate::LocalFileCommitError,
    /// Whether the writer remains owned for a retry.
    retryable: bool,
}

impl LocalStreamCommitError {
    /// Creates the error returned by the writer adapter after a commit attempt.
    ///
    /// # Parameters
    ///
    /// * `error` - Structured publication error produced by the worker.
    /// * `retryable` - Whether the writer remains available for another commit.
    pub(in crate::operation) fn new(error: crate::LocalFileCommitError, retryable: bool) -> Self {
        Self { error, retryable }
    }

    /// Returns the structured local file commit error.
    pub fn commit_error(&self) -> &crate::LocalFileCommitError {
        &self.error
    }

    /// Whether the async writer handle still owns a retryable native writer.
    pub const fn retryable(&self) -> bool {
        self.retryable
    }
}

impl std::fmt::Display for LocalStreamCommitError {
    /// Formats the underlying structured publication error.
    ///
    /// # Parameters
    ///
    /// * `formatter` - Destination formatter for the displayed error.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for LocalStreamCommitError {
    /// Returns the underlying publication error as the source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::LocalStreamCommitError;
    use crate::LocalFileCommitError;
    use crate::LocalFileError;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalWriteFailureState;

    #[test]
    fn test_commit_error_exposes_retry_state_and_preserves_native_source() {
        let source = LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Commit);
        let commit = LocalFileCommitError::new(source, LocalWriteFailureState::NotPublished, None);
        let error = LocalStreamCommitError::new(commit, true);

        assert!(error.retryable());
        assert_eq!(error.commit_error().state(), LocalWriteFailureState::NotPublished);
        assert_eq!(error.to_string(), error.commit_error().to_string());
        assert_eq!(
            Error::source(&error).unwrap().to_string(),
            error.commit_error().to_string()
        );

        let non_retryable = LocalStreamCommitError::new(
            LocalFileCommitError::new(
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Commit),
                LocalWriteFailureState::Indeterminate,
                None,
            ),
            false,
        );
        assert!(!non_retryable.retryable());
    }
}
