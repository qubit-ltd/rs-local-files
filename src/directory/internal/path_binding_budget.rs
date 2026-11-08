// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Shared byte budget for path resolution and retained ancestor anchors.
#[derive(Debug)]
pub(crate) struct PathBindingBudget {
    /// Optional maximum total path-byte charge; `None` leaves the budget
    /// unbounded.
    limit: Option<usize>,
    /// Bytes charged so far across the current path and its retained ancestors.
    used: usize,
}

impl PathBindingBudget {
    /// Creates an empty budget with the optional limit applied to all charges.
    ///
    /// # Parameters
    ///
    /// - `limit`: Maximum cumulative bytes, or `None` for no configured
    ///   maximum.
    ///
    /// # Returns
    ///
    /// A budget with zero bytes charged.
    #[inline]
    pub(crate) const fn new(limit: Option<usize>) -> Self {
        Self { limit, used: 0 }
    }

    /// Charges path bytes and reports a resource-limit error when accounting
    /// fails.
    ///
    /// The counter is updated only when addition succeeds. If the configured
    /// limit is exceeded, the attempted charge remains accounted for so callers
    /// cannot reuse capacity after a rejected path binding.
    ///
    /// # Parameters
    ///
    /// - `bytes`: Number of path bytes to add to the cumulative charge.
    /// - `operation`: Filesystem operation associated with this accounting
    ///   step.
    /// - `path`: Path to attach to a resource-limit error.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` after charging bytes when the configured limit is not
    /// exceeded.
    ///
    /// # Errors
    ///
    /// Returns a resource-limit error if accounting overflows or exceeds
    /// `limit`.
    pub(crate) fn debit(&mut self, bytes: usize, operation: LocalFileOperation, path: &Path) -> LocalResult<()> {
        self.used = self.used.checked_add(bytes).ok_or_else(|| {
            LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(path.to_path_buf())
        })?;
        if self.limit.is_some_and(|limit| self.used > limit) {
            return Err(LocalFileError::new(LocalFileErrorKind::ResourceLimit, operation).with_path(path.to_path_buf()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::PathBindingBudget;
    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;

    #[test]
    fn test_path_binding_budget_overflow_preserves_accounting_and_error_context() {
        let mut budget = PathBindingBudget::new(None);
        budget
            .debit(usize::MAX, LocalFileOperation::Metadata, Path::new("ancestor"))
            .expect("the largest representable byte charge should fit an unbounded budget");
        let path = Path::new("ancestor/child");

        let error = budget
            .debit(1, LocalFileOperation::Metadata, path)
            .expect_err("unbounded accounting must still reject overflow");

        assert_eq!(budget.used, usize::MAX);
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(path));
        budget
            .debit(0, LocalFileOperation::Metadata, path)
            .expect("overflow rejection must leave the previous valid counter usable");
    }
}
