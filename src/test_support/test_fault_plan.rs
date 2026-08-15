//! Instance-local deterministic fault plans.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

use super::TestFaultPoint;

#[derive(Debug)]
struct TestFault {
    point: TestFaultPoint,
    kind: io::ErrorKind,
}

/// FIFO fault plan scoped to one filesystem instance.
#[derive(Clone, Debug)]
pub struct TestFaultPlan {
    faults: Arc<Mutex<VecDeque<TestFault>>>,
}

impl TestFaultPlan {
    /// Creates a plan that fails once at `point` with `kind`.
    pub fn fail_once(point: TestFaultPoint, kind: io::ErrorKind) -> Self {
        let mut faults = VecDeque::new();
        faults.push_back(TestFault { point, kind });
        Self {
            faults: Arc::new(Mutex::new(faults)),
        }
    }

    /// Takes the next matching fault, if one remains.
    pub(crate) fn take(&self, point: TestFaultPoint) -> Option<io::Error> {
        let mut faults = self
            .faults
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = faults.iter().position(|fault| fault.point == point)?;
        let fault = faults.remove(index)?;
        Some(io::Error::new(
            fault.kind,
            "injected local filesystem fault",
        ))
    }
}
