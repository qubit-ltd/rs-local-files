// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private types shared by staged-directory implementation modules.

mod owner_policy_entry;
mod retained_owner_policy_entry;
pub(super) mod tree_sync_budget;

pub(super) use owner_policy_entry::OwnerPolicyEntry;
pub(super) use retained_owner_policy_entry::RetainedOwnerPolicyEntry;
