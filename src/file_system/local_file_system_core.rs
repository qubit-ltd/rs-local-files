//! Shared immutable state retained by filesystem clones.

use std::sync::Arc;

use super::LocalCopyLimits;
use super::LocalWalkLimits;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemProtocols;
use crate::authority::Authority;
use crate::path::LocalPaths;

/// Immutable state shared by all clones of a configured filesystem.
#[derive(Debug)]
pub(crate) struct LocalFileSystemCore {
    pub(crate) authority: Option<Arc<Authority>>,
    pub(crate) paths: LocalPaths,
    pub(crate) protocols: LocalFileSystemProtocols,
    pub(crate) limits: LocalFileSystemLimits,
    pub(crate) walk_limits: LocalWalkLimits,
    pub(crate) copy_limits: LocalCopyLimits,
    #[cfg(feature = "test-support")]
    pub(crate) test_faults: Option<crate::TestFaultPlan>,
}

impl LocalFileSystemCore {
    /// Returns an injected error for one operation boundary, when configured.
    pub(crate) fn fail_if_requested(
        &self,
        point: crate::test_support::TestFaultPoint,
    ) -> std::io::Result<()> {
        #[cfg(feature = "test-support")]
        {
            if let Some(error) =
                self.test_faults.as_ref().and_then(|plan| plan.take(point))
            {
                return Err(error);
            }
        }
        let _ = point;
        Ok(())
    }
}
