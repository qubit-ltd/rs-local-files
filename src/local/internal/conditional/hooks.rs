// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Named deterministic checkpoint actions shared by generic and fake backends.

use std::io;

use super::ConditionalHookAction;
use super::ConditionalPoint;

/// Single-use deterministic actions attached to named protocol checkpoints.
#[derive(Clone, Debug, Default)]
pub(crate) struct ConditionalHooks {
    actions: std::rc::Rc<std::cell::RefCell<Vec<(ConditionalPoint, ConditionalHookAction)>>>,
}

impl ConditionalHooks {
    pub(crate) fn sequence(actions: Vec<(ConditionalPoint, ConditionalHookAction)>) -> Self {
        Self {
            actions: std::rc::Rc::new(std::cell::RefCell::new(actions)),
        }
    }

    pub(crate) fn fail_once(point: ConditionalPoint, kind: io::ErrorKind) -> Self {
        Self::sequence(vec![(point, ConditionalHookAction::Fail(kind))])
    }

    pub(crate) fn partial_publish_once() -> Self {
        Self::sequence(vec![(
            ConditionalPoint::Publish,
            ConditionalHookAction::PartialPublish,
        )])
    }

    pub(crate) fn take(&self, point: ConditionalPoint) -> Option<ConditionalHookAction> {
        let mut actions = self.actions.borrow_mut();
        let index = actions
            .iter()
            .position(|(candidate, _)| *candidate == point)?;
        Some(actions.remove(index).1)
    }
}
