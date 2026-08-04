// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
// =============================================================================
//! Directory-handle budget policies.

/// Controls what a recursive walker does after reaching its open-handle
/// budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalDirectoryReopenPolicy {
    /// Return [`crate::LocalFileErrorKind::ResourceLimit`].
    Fail,
    /// Close active readers and reopen them while resuming the walk.
    Reopen,
}
