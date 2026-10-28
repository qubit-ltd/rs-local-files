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
