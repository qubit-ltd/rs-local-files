// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Named deterministic checkpoint actions shared by generic and fake backends.

use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use super::ConditionalHookAction;
use super::ConditionalPoint;

#[cfg(all(feature = "digest", feature = "test-support"))]
type HookCallback = Arc<Mutex<Option<Box<dyn FnOnce() -> io::Result<()> + Send>>>>;

/// Single-use deterministic actions attached to named protocol checkpoints.
#[derive(Clone, Default)]
pub(crate) struct ConditionalHooks {
    actions: Arc<Mutex<Vec<(ConditionalPoint, ConditionalHookAction)>>>,
    #[cfg(all(feature = "digest", feature = "test-support"))]
    callbacks: Arc<Mutex<Vec<(ConditionalPoint, HookCallback)>>>,
}

impl std::fmt::Debug for ConditionalHooks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ConditionalHooks").finish_non_exhaustive()
    }
}

impl ConditionalHooks {
    #[cfg(all(feature = "digest", feature = "test-support"))]
    pub(crate) fn from_test(hooks: crate::test_support::TestConditionalHooks) -> Self {
        use crate::test_support::TestConditionalPoint as PublicPoint;

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

    #[cfg(all(feature = "digest", feature = "test-support"))]
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

    #[cfg(test)]
    pub(crate) fn sequence(actions: Vec<(ConditionalPoint, ConditionalHookAction)>) -> Self {
        Self {
            actions: Arc::new(Mutex::new(actions)),
            #[cfg(all(feature = "digest", feature = "test-support"))]
            callbacks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_once(point: ConditionalPoint, kind: io::ErrorKind) -> Self {
        Self::sequence(vec![(point, ConditionalHookAction::Fail(kind))])
    }

    #[cfg(test)]
    pub(crate) fn partial_publish_once() -> Self {
        Self::sequence(vec![(ConditionalPoint::Publish, ConditionalHookAction::PartialPublish)])
    }

    pub(crate) fn take(&self, point: ConditionalPoint) -> Option<ConditionalHookAction> {
        let mut actions = self.actions.lock().expect("conditional hook mutex poisoned");
        let index = actions.iter().position(|(candidate, _)| *candidate == point)?;
        Some(actions.remove(index).1)
    }

    #[cfg(all(feature = "digest", feature = "test-support"))]
    pub(crate) fn set_callback(&mut self, point: ConditionalPoint, callback: HookCallback) {
        self.callbacks
            .lock()
            .expect("conditional hook mutex poisoned")
            .push((point, callback));
    }

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
