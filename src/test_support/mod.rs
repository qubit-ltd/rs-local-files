// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic fault injection for downstream adapter tests.
//!
//! This module is available only with the `test-support` feature. It is not a
//! production API and does not carry the crate's normal semver compatibility
//! promise. Instance-local plans validate isolated facade behavior; the
//! process-local guard exists for native boundary and recovery-state tests that
//! cannot be triggered through ordinary inputs.

mod internal;
mod test_fault_plan;
mod test_fault_point;

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_fault_plan::TestFaultPlan;
pub use test_fault_point::TestFaultPoint;

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::TestFaultGuard;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::install_test_fault;
