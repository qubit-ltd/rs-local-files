// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private path and directory operations.
// qubit-style: allow source-test-pair

mod path_management;

pub(crate) use path_management::absolute_path;
pub(crate) use path_management::add_path_context;
pub(super) use path_management::canonicalize_existing_prefix;
pub(crate) use path_management::ensure_parent_path;
pub(crate) use path_management::ensure_parent_path_with_sync_dirs;
