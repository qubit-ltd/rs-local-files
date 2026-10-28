// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::outcome::LocalTreeAccessState;
use crate::policy::LocalOwnerAccessPolicy;
/// Reports the access policy confirmed for a staged tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalTreeAccessOutcome {
    state: LocalTreeAccessState,
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}
impl LocalTreeAccessOutcome {
    pub(crate) const fn new(state: LocalTreeAccessState) -> Self {
        Self { state, owner_access_policy: None }
    }
    pub(crate) const fn with_owner_policy(state: LocalTreeAccessState, policy: LocalOwnerAccessPolicy) -> Self {
        Self { state, owner_access_policy: Some(policy) }
    }
    /// Returns the access state confirmed by the operation.
    pub const fn access_state(self) -> LocalTreeAccessState {
        self.state
    }
    /// Returns the exact owner policy confirmed for this tree, when applied.
    pub const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }
}
