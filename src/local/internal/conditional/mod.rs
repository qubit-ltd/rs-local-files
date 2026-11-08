// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private contracts shared by conditional-write backends.

mod candidate_copy;
mod cleanup;
mod commit;
mod conditional_backend;
mod conditional_hook_action;
mod conditional_point;
mod digest_reader;
mod entry;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod hook_callback;
mod hooks;
mod native_attempt;
mod recovery_access;

#[cfg(feature = "digest")]
pub(crate) use backend_dispatch::ConditionalWriterBackend;
#[cfg(feature = "digest")]
mod backend_dispatch;

#[cfg(test)]
mod fake_backend_tests;
#[cfg(test)]
mod state_tests;

#[cfg(unix)]
pub(super) mod unix;
#[cfg(not(any(unix, windows)))]
mod unsupported;
#[cfg(windows)]
pub(super) mod windows;

#[cfg(test)]
pub(crate) use commit::commit_conditional;
pub(crate) use commit::commit_conditional_with_directory;
pub(crate) use conditional_backend::ConditionalBackend;
pub(crate) use conditional_hook_action::ConditionalHookAction;
pub(crate) use conditional_point::ConditionalPoint;
pub(crate) use entry::ConditionalEntry;
pub(crate) use hooks::ConditionalHooks;
pub(crate) use native_attempt::NativeAttempt;
pub(crate) use recovery_access::RecoveryAccess;
