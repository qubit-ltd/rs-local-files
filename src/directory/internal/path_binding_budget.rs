// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use crate::LocalResult;

/// Shared byte budget for path resolution and retained ancestor anchors.
#[derive(Debug)]
pub(crate) struct PathBindingBudget {
    limit: Option<usize>,
    used: usize,
}

impl PathBindingBudget {
    pub(crate) const fn new(limit: Option<usize>) -> Self {
        Self { limit, used: 0 }
    }

    pub(crate) fn debit(&mut self, bytes: usize, operation: crate::LocalFileOperation, path: &Path) -> LocalResult<()> {
        self.used = self.used.checked_add(bytes).ok_or_else(|| {
            crate::LocalFileError::new(crate::LocalFileErrorKind::ResourceLimit, operation)
                .with_path(path.to_path_buf())
        })?;
        if self.limit.is_some_and(|limit| self.used > limit) {
            return Err(
                crate::LocalFileError::new(crate::LocalFileErrorKind::ResourceLimit, operation)
                    .with_path(path.to_path_buf()),
            );
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
    fn test_path_binding_budget_charges_ancestors_against_one_shared_limit() {
        let mut budget = PathBindingBudget::new(Some(5));
        budget
            .debit(2, LocalFileOperation::Metadata, Path::new("a"))
            .expect("first ancestor should fit");
        budget
            .debit(3, LocalFileOperation::Metadata, Path::new("a/b"))
            .expect("exact shared limit should fit");
        let path = Path::new("a/b/c");

        let error = budget
            .debit(1, LocalFileOperation::Metadata, path)
            .expect_err("next anchor must exceed shared capacity");

        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(path));
    }

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
