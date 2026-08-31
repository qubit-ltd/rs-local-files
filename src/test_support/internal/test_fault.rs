// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private fault-plan entries.

use std::io;

use super::super::TestFaultPoint;

/// One operation boundary and error kind retained by a fault plan.
#[derive(Debug)]
pub(crate) struct TestFault {
    /// Operation boundary at which the fault is injected.
    pub(crate) point: TestFaultPoint,
    /// Native error kind returned at the boundary.
    pub(crate) kind: io::ErrorKind,
}
