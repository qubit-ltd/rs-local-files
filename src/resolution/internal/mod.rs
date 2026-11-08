// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod path_binding_deadline;
mod resolution_contract;
#[cfg(any(windows, test))]
pub(super) mod unc_segment;
pub(crate) use path_binding_deadline::checked_path_binding_deadline;
pub(crate) use resolution_contract::ResolutionContract;
