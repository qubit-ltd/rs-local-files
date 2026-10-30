// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-local, one-shot failures at native I/O contract boundaries.

use std::cell::RefCell;
use std::io;
use std::marker::PhantomData;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NativeIoPoint {
    CloneAuthority,
    OpenCursor,
    CloneCursor,
    CursorMetadata,
    CursorDescend,
    Observation,
    RootMetadata,
    EntryMetadata,
    RootOpen,
    ReaderMetadata,
    Read,
    FileWrite,
    SyncAll,
    #[cfg(unix)]
    HandleMetadata,
    RandomName,
    DeleteMetadata,
    DeleteReadDirectory,
    DeleteRemoveFile,
    DeleteRemoveDirectory,
}

struct FaultPlan {
    point: NativeIoPoint,
    remaining: usize,
    fired: bool,
}

thread_local! {
    static PLAN: RefCell<Option<FaultPlan>> = const { RefCell::new(None) };
}

pub(crate) struct NativeIoFaultGuard {
    previous: Option<FaultPlan>,
    thread: PhantomData<Rc<()>>,
}

impl NativeIoFaultGuard {
    pub(crate) fn fail_on(point: NativeIoPoint, occurrence: usize) -> Self {
        assert!(occurrence > 0, "native operation occurrences start at one");
        let previous = PLAN.with(|plan| {
            plan.replace(Some(FaultPlan {
                point,
                remaining: occurrence,
                fired: false,
            }))
        });
        Self {
            previous,
            thread: PhantomData,
        }
    }

    pub(crate) fn fired(&self) -> bool {
        PLAN.with(|plan| plan.borrow().as_ref().is_some_and(|plan| plan.fired))
    }
}

impl Drop for NativeIoFaultGuard {
    fn drop(&mut self) {
        PLAN.with(|plan| plan.replace(self.previous.take()));
    }
}

pub(crate) fn check(point: NativeIoPoint) -> io::Result<()> {
    PLAN.with(|plan| {
        let mut plan = plan.borrow_mut();
        let Some(plan) = plan.as_mut() else { return Ok(()) };
        if plan.point != point || plan.fired {
            return Ok(());
        }
        plan.remaining -= 1;
        if plan.remaining == 0 {
            plan.fired = true;
            Err(io::Error::other("injected native I/O contract failure"))
        } else {
            Ok(())
        }
    })
}

#[cfg(unix)]
pub(crate) fn metadata(file: &std::fs::File) -> io::Result<std::fs::Metadata> {
    check(NativeIoPoint::HandleMetadata)?;
    file.metadata()
}

pub(crate) fn random_fill(bytes: &mut [u8]) -> Result<(), getrandom::Error> {
    if check(NativeIoPoint::RandomName).is_err() {
        return Err(getrandom::Error::UNSUPPORTED);
    }
    getrandom::fill(bytes)
}

#[test]
fn test_fault_plan_is_one_shot_and_nested_guards_restore_the_parent_plan() {
    let outer = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 2);
    check(NativeIoPoint::CloneAuthority).expect("first parent operation should succeed");
    {
        let inner = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
        check(NativeIoPoint::CloneAuthority).expect("other operation should not consume the child plan");
        assert!(check(NativeIoPoint::Observation).is_err());
        assert!(inner.fired());
        check(NativeIoPoint::Observation).expect("a fault should fire only once");
    }
    assert!(check(NativeIoPoint::CloneAuthority).is_err());
    assert!(outer.fired());
    drop(outer);
    check(NativeIoPoint::CloneAuthority).expect("dropping the guard should restore normal native I/O");
}
