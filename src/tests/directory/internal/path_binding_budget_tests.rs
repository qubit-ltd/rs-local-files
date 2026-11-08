// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::LocalFileOperation;
use crate::directory::binding::PathBindingBudget;
use crate::error::LocalFileErrorKind;

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
