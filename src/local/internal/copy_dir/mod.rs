// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Private recursive directory-copy pipeline.

mod copy_dir_frame;
mod copy_dir_result;
mod destination;
mod error;
mod facade;
mod namespace_race;
mod source;
mod staged_copy;
mod statistics_overflow;
mod traversal;

pub(crate) use facade::copy_dir_all_with_paths;
