// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic, instance-local fault injection for tests.

mod internal;
mod test_fault_plan;
mod test_fault_point;

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_fault_plan::TestFaultPlan;
pub use test_fault_point::TestFaultPoint;
