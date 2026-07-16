// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Private recursive directory-copy pipeline.

mod destination;
mod error;
mod facade;
mod source;
mod staged_copy;
mod traversal;

pub(crate) use facade::copy_dir_all_with_paths;
