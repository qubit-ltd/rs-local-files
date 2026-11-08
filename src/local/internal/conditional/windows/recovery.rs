// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native ReplaceFileW failure classes used alongside observed receipts.

mod replace_failure;

#[cfg(test)]
pub(super) use replace_failure::ReplaceFailure;
pub(super) use replace_failure::classify_replace_failure;
