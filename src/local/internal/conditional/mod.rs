// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private contracts shared by conditional-write backends.

mod backend;
mod entry;

pub(crate) use backend::ConditionalBackend;
pub(crate) use backend::ConditionalHooks;
pub(crate) use backend::ConditionalPoint;
pub(crate) use backend::NativeAttempt;
pub(crate) use entry::ConditionalEntry;
pub(crate) use entry::RecoveryAccess;
