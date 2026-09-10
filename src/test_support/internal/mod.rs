// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private helpers for deterministic fault plans.

mod test_fault;

pub(super) use test_fault::TestFault;

#[cfg(feature = "test-support")]
mod temp_cleanup_fault;
#[cfg(feature = "test-support")]
pub(crate) use temp_cleanup_fault::temp_cleanup_add_child;
#[cfg(feature = "test-support")]
pub(crate) use temp_cleanup_fault::temp_cleanup_before_remove;
#[cfg(feature = "test-support")]
pub(crate) use temp_cleanup_fault::temp_cleanup_deadline_expired;
#[cfg(feature = "test-support")]
pub(crate) use temp_cleanup_fault::temp_cleanup_metadata;
#[cfg(all(unix, feature = "test-support"))]
pub(crate) use temp_cleanup_fault::temp_cleanup_replace_observed_entry;
