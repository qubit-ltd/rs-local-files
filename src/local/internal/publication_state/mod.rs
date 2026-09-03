// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Publication-state translation shared by local operations.

mod copy_failure;
mod durability;
mod rename_failure;

pub(crate) use copy_failure::copy_failure_indeterminate;
pub(crate) use copy_failure::copy_failure_published;
pub(crate) use copy_failure::copy_failure_unchanged;
pub(crate) use durability::published_durability;
pub(crate) use rename_failure::rename_failure_after_native_attempt;
pub(crate) use rename_failure::rename_failure_renamed;
pub(crate) use rename_failure::rename_failure_unchanged;
