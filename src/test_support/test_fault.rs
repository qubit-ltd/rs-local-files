//! Fault plans owned by one filesystem instance.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;

/// Native operation boundary at which a deterministic test fault is injected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestFaultPoint {
    /// Metadata observation.
    Metadata,
    /// Directory walk opening.
    WalkOpen,
    /// Copy source read.
    CopyRead,
    /// Copy destination write.
    CopyWrite,
    /// Publication flush.
    PublicationFlush,
    /// Publication file synchronization.
    PublicationSyncFile,
    /// Publication installation.
    PublicationInstall,
    /// Publication parent synchronization.
    PublicationSyncParent,
    /// Publication cleanup.
    PublicationCleanup,
    /// Temporary-resource identity verification.
    TempIdentity,
    /// Temporary-resource cleanup.
    TempCleanup,
}

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
