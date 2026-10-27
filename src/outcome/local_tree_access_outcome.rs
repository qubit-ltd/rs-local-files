// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::outcome::LocalTreeAccessState;
/// Reports the access policy confirmed for a staged tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTreeAccessOutcome {
    state: LocalTreeAccessState,
}
impl LocalTreeAccessOutcome {
    pub(crate) const fn new(state: LocalTreeAccessState) -> Self {
        Self { state }
    }
    /// Returns the access state confirmed by the operation.
    pub const fn access_state(self) -> LocalTreeAccessState {
        self.state
    }
}
