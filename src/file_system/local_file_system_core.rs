//! Shared immutable state retained by filesystem clones.

use crate::LocalFileSystemLimits;
use crate::LocalFileSystemProtocols;
use crate::local::LocalNamespace;

/// Immutable state shared by all clones of a configured filesystem.
#[derive(Debug)]
pub(crate) struct LocalFileSystemCore {
    pub(crate) namespace: LocalNamespace,
    pub(crate) protocols: LocalFileSystemProtocols,
    pub(crate) limits: LocalFileSystemLimits,
    #[cfg(feature = "test-support")]
    pub(crate) test_faults: Option<crate::TestFaultPlan>,
}

impl LocalFileSystemCore {
    /// Returns an injected error for one operation boundary, when configured.
    pub(crate) fn fail_if_requested(&self, point: crate::test_support::TestFaultPoint) -> std::io::Result<()> {
        #[cfg(feature = "test-support")]
        {
            if let Some(error) = self.test_faults.as_ref().and_then(|plan| plan.take(point)) {
                return Err(error);
            }
        }
        let _ = point;
        Ok(())
    }
}
