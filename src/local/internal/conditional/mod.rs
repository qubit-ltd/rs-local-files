// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private contracts shared by conditional-write backends.

mod backend;
mod candidate_copy;
mod cleanup;
mod commit;
mod digest_reader;
mod entry;
mod hooks;

#[cfg(test)]
mod fake_backend;
#[cfg(test)]
mod state_tests;

#[cfg(unix)]
mod unix;
#[cfg(not(any(unix, windows)))]
mod unsupported;
#[cfg(windows)]
mod windows;

pub(crate) use backend::ConditionalBackend;
pub(crate) use backend::ConditionalHookAction;
pub(crate) use backend::ConditionalPoint;
pub(crate) use backend::NativeAttempt;
pub(crate) use entry::ConditionalEntry;
pub(crate) use entry::RecoveryAccess;
pub(crate) use hooks::ConditionalHooks;
