// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix conditional publication using retained directory descriptors.

mod entry_access;
mod install;
mod metadata;
mod namespace;
mod private_entry_guard;
mod private_entry_setup_error;
mod recovery;
#[path = "unix_conditional_backend.rs"]
mod unix_conditional_backend;

pub(super) use unix_conditional_backend::UnixConditionalBackend;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) mod test_support {
    use std::cell::RefCell;
    use std::io;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(super) enum FailurePoint {
        GuardIdentityMetadata,
        CandidateMetadata,
        EntryIdentity,
        EntryParentClone,
        CandidateCopyRegistration,
        ExchangeEioBeforeSuccess,
        ExchangeEioAfterSuccess,
        CandidateIdentityAfterExchange,
        InstalledIdentityAfterExchange,
        StagedIdentityAfterExchange,
    }

    thread_local! {
        static FAILURE_POINTS: RefCell<Vec<FailurePoint>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn fail_next(point: FailurePoint) {
        FAILURE_POINTS.with(|slot| slot.borrow_mut().push(point));
    }

    pub(super) fn fail_if_requested(point: FailurePoint) -> io::Result<()> {
        let should_fail = FAILURE_POINTS.with(|slot| {
            let mut points = slot.borrow_mut();
            if let Some(index) = points.iter().position(|pending| *pending == point) {
                points.remove(index);
                true
            } else {
                false
            }
        });
        if should_fail {
            Err(io::Error::other(format!("injected {point:?} failure")))
        } else {
            Ok(())
        }
    }

    pub(super) fn assert_no_pending_failures() {
        FAILURE_POINTS.with(|slot| {
            assert!(
                slot.borrow().is_empty(),
                "unconsumed Unix conditional fault points remain"
            );
        });
    }
}
