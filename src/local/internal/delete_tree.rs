// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared post-order recursive deletion.

use std::io;
use std::time::Instant;

use super::delete_backend::DeleteBackend;
use super::delete_budget::DeleteBudget;
use super::delete_work::DeleteWork;
use super::directory_mutation_error::directory_mutation_error;
use crate::LocalDeleteOptions;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Removes a directory tree while preserving budgets, failure paths, and state.
pub(crate) fn remove_directory_tree<B: DeleteBackend>(
    backend: &B,
    root: &B::Path,
    options: LocalDeleteOptions,
    started_at: Instant,
) -> LocalResult<()> {
    let mut changed = false;
    let mut budget = DeleteBudget::new(options, started_at);
    let operation = LocalFileOperation::DeleteDirectory;
    let fail = |path: &B::Path, changed: bool, source: io::Error| {
        directory_mutation_error(operation, backend.path(path), changed, source)
    };

    budget
        .discover(0)
        .and_then(|()| budget.reserve_path(backend.path(root)))
        .map_err(|error| fail(root, false, error))?;
    let mut work = vec![(DeleteWork::Inspect(root.clone()), 0_usize)];
    while let Some((item, depth)) = work.pop() {
        let path = match &item {
            DeleteWork::Inspect(path) | DeleteWork::RemoveDirectory(path) => path,
        };
        budget.release_path(backend.path(path));
        budget.check_deadline().map_err(|error| fail(path, changed, error))?;
        match item {
            DeleteWork::Inspect(path) => {
                let metadata = backend.metadata(&path).map_err(|error| fail(&path, changed, error))?;
                let is_directory = backend.is_directory(&metadata);
                if depth == 0 && !is_directory {
                    return Err(fail(&path, changed, io::Error::from(io::ErrorKind::NotADirectory)));
                }
                if is_directory {
                    let mut reader = backend
                        .open_directory(&path)
                        .map_err(|error| fail(&path, changed, error))?;
                    budget
                        .reserve_path(backend.path(&path))
                        .map_err(|error| fail(&path, changed, error))?;
                    work.push((DeleteWork::RemoveDirectory(path.clone()), depth));
                    let children_start = work.len();
                    loop {
                        budget.check_deadline().map_err(|error| fail(&path, changed, error))?;
                        let Some(child) = backend
                            .next_child(&path, &mut reader)
                            .map_err(|error| fail(&path, changed, error))?
                        else {
                            break;
                        };
                        budget
                            .discover(depth + 1)
                            .and_then(|()| budget.reserve_path(backend.path(&child)))
                            .map_err(|error| fail(&child, changed, error))?;
                        work.push((DeleteWork::Inspect(child), depth + 1));
                    }
                    work[children_start..].reverse();
                } else {
                    budget.check_deadline().map_err(|error| fail(&path, changed, error))?;
                    backend
                        .before_remove(&path)
                        .map_err(|error| fail(&path, changed, error))?;
                    backend
                        .remove_non_directory(&path, &metadata)
                        .map_err(|error| fail(&path, changed, error))?;
                    changed = true;
                }
            }
            DeleteWork::RemoveDirectory(path) => {
                backend
                    .before_remove(&path)
                    .map_err(|error| fail(&path, changed, error))?;
                backend
                    .remove_empty_directory(&path)
                    .map_err(|error| fail(&path, changed, error))?;
                changed = true;
            }
        }
    }
    Ok(())
}
