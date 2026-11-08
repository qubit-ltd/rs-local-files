// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Named deterministic checkpoint actions shared by generic and fake backends.

#[cfg(any(test, all(feature = "digest", feature = "test-support")))]
use std::io;
#[cfg(any(test, all(feature = "digest", feature = "test-support")))]
use std::sync::Arc;
#[cfg(any(test, all(feature = "digest", feature = "test-support")))]
use std::sync::Mutex;

use super::super::ConditionalHookAction;
use super::super::ConditionalPoint;
#[cfg(all(feature = "digest", feature = "test-support"))]
use super::super::hook_callback::HookCallback;
#[cfg(all(feature = "digest", feature = "test-support"))]
use crate::test_support::TestConditionalPoint as PublicPoint;

/// Single-use deterministic actions attached to named protocol checkpoints.
#[derive(Clone, Default)]
pub(crate) struct ConditionalHooks {
    /// Queues deterministic actions for checkpoints, when test hooks are
    /// enabled.
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    actions: Arc<Mutex<Vec<(ConditionalPoint, ConditionalHookAction)>>>,
    /// Stores one-shot callbacks for checkpoints in test-support builds.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    callbacks: Arc<Mutex<Vec<(ConditionalPoint, HookCallback)>>>,
}

/// Formats the hook set without exposing queued actions or callback internals.
impl std::fmt::Debug for ConditionalHooks {
    /// Writes a stable summary without exposing synchronization state.
    ///
    /// # Parameters
    ///
    /// * `formatter` is the destination for the non-exhaustive debug summary.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ConditionalHooks").finish_non_exhaustive()
    }
}

impl ConditionalHooks {
    /// Converts a public test hook into the corresponding internal checkpoint
    /// callback.
    ///
    /// # Parameters
    ///
    /// * `hooks` is the public test-support hook to translate.
    ///
    /// # Returns
    ///
    /// The internal hook set containing the translated one-shot callback.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    #[must_use]
    pub(crate) fn from_test(hooks: crate::test_support::TestConditionalHooks) -> Self {
        let (point, callback) = hooks.into_parts();
        let point = match point {
            PublicPoint::BeforeWrite => ConditionalPoint::Write,
            PublicPoint::BeforeCandidateCopy => ConditionalPoint::PrepareCandidateCopy,
            PublicPoint::AfterCandidateCopy => ConditionalPoint::CandidateCopyCreated,
            PublicPoint::BeforePrecheck => ConditionalPoint::Precheck,
            PublicPoint::AfterPrecheck => ConditionalPoint::AfterPrecheck,
            PublicPoint::BeforePublish => ConditionalPoint::Publish,
            PublicPoint::AfterPublish => ConditionalPoint::AfterPublish,
            PublicPoint::BeforeVerifyDisplaced => ConditionalPoint::VerifyDisplaced,
            PublicPoint::BeforeVerifyInstalled => ConditionalPoint::VerifyInstalled,
            PublicPoint::AfterRollbackPrecheck => ConditionalPoint::RollbackValidated,
            PublicPoint::BeforeRollbackExchange => ConditionalPoint::RollbackExchange,
            PublicPoint::AfterRollback => ConditionalPoint::AfterRollbackSyscall,
            PublicPoint::BeforeRequiredFileSync => ConditionalPoint::BeforeRequiredFileSync,
            PublicPoint::BeforeInstalledFileSync => ConditionalPoint::SyncInstalled,
            PublicPoint::BeforeRequiredParentSync => ConditionalPoint::BeforeRequiredParentSync,
            PublicPoint::AfterSaveComplete => ConditionalPoint::SaveComplete,
            PublicPoint::BeforeCleanupOld => ConditionalPoint::CleanupOld,
            PublicPoint::BeforeCleanupCopy => ConditionalPoint::CleanupCopy,
            PublicPoint::BeforeCleanupSync => ConditionalPoint::CleanupSync,
        };
        let mut result = Self::default();
        result.set_callback(point, callback);
        result
    }

    /// Clones this hook set and appends a callback supplied by test support.
    ///
    /// # Parameters
    ///
    /// * `hooks` is the additional public test-support hook.
    ///
    /// # Returns
    ///
    /// A new hook set with the current queued state and the additional
    /// callback.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    #[must_use]
    pub(crate) fn with_test(&self, hooks: crate::test_support::TestConditionalHooks) -> Self {
        let mut result = Self {
            actions: Arc::new(Mutex::new(
                self.actions.lock().expect("conditional hook mutex poisoned").clone(),
            )),
            callbacks: Arc::new(Mutex::new(
                self.callbacks.lock().expect("conditional hook mutex poisoned").clone(),
            )),
        };
        let addition = Self::from_test(hooks);
        let (point, callback) = addition.callbacks.lock().expect("conditional hook mutex poisoned")[0].clone();
        result.set_callback(point, callback);
        result
    }

    /// Creates a hook set with actions consumed in the supplied checkpoint
    /// order.
    ///
    /// # Parameters
    ///
    /// * `actions` is the ordered list of checkpoint actions to queue.
    ///
    /// # Returns
    ///
    /// A hook set that shares the queued actions with its clones.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn sequence(actions: Vec<(ConditionalPoint, ConditionalHookAction)>) -> Self {
        Self {
            actions: Arc::new(Mutex::new(actions)),
            #[cfg(all(feature = "digest", feature = "test-support"))]
            callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Creates a hook set that fails once at the selected checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` identifies the checkpoint to fail.
    /// * `kind` is the I/O error kind returned at that checkpoint.
    ///
    /// # Returns
    ///
    /// A hook set with one queued failure action.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn fail_once(point: ConditionalPoint, kind: io::ErrorKind) -> Self {
        Self::sequence(vec![(point, ConditionalHookAction::Fail(kind))])
    }

    /// Creates a hook set that simulates a partial publish once.
    ///
    /// # Returns
    ///
    /// A hook set with one partial-publish action.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn partial_publish_once() -> Self {
        Self::sequence(vec![(ConditionalPoint::Publish, ConditionalHookAction::PartialPublish)])
    }

    /// Adds a one-shot callback for the selected checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` identifies the checkpoint that triggers the callback.
    /// * `callback` is the shared one-shot callback to enqueue.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    pub(crate) fn set_callback(&mut self, point: ConditionalPoint, callback: HookCallback) {
        self.callbacks
            .lock()
            .expect("conditional hook mutex poisoned")
            .push((point, callback));
    }

    /// Removes and returns the first queued action matching `point`.
    ///
    /// # Parameters
    ///
    /// * `point` identifies the checkpoint whose action should be consumed.
    ///
    /// # Returns
    ///
    /// The consumed action, or `None` when no action matches or hooks are
    /// disabled.
    pub(crate) fn take(&self, point: ConditionalPoint) -> Option<ConditionalHookAction> {
        #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
        {
            let mut actions = self.actions.lock().expect("conditional hook mutex poisoned");
            let index = actions.iter().position(|(candidate, _)| *candidate == point)?;
            Some(actions.remove(index).1)
        }
        #[cfg(not(any(test, all(feature = "digest", feature = "test-support"))))]
        {
            let _ = point;
            None
        }
    }

    /// Runs and consumes the callback registered for `point`, if one exists.
    ///
    /// # Parameters
    ///
    /// * `point` identifies the checkpoint callback to consume.
    ///
    /// # Returns
    ///
    /// `Ok(())` when no callback is registered or it completes successfully.
    ///
    /// # Errors
    ///
    /// Returns the I/O error produced by the callback.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    pub(crate) fn run_callback(&self, point: ConditionalPoint) -> io::Result<()> {
        let callback = {
            let mut callbacks = self.callbacks.lock().expect("conditional hook mutex poisoned");
            let Some(index) = callbacks.iter().position(|(candidate, _)| *candidate == point) else {
                return Ok(());
            };
            callbacks.remove(index).1
        };
        let action = callback.lock().expect("test hook mutex poisoned").take();
        if let Some(action) = action {
            action()?;
        }
        Ok(())
    }
}

#[cfg(not(any(test, all(feature = "digest", feature = "test-support"))))]
const _: () = assert!(std::mem::size_of::<ConditionalHooks>() == 0);
