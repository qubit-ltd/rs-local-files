// qubit-style: allow all -- temporary-resource behavior is covered by resource
// integration tests.
//! Root-descriptor-bound temporary-resource storage.

use std::{path::PathBuf, sync::Arc};

/// Retains the exact root authority used to create a temporary descendant.
#[derive(Debug)]
pub(crate) struct RootedTempResourceBackend {
    /// Open root descriptor/handle; its diagnostic path is never authority.
    pub(crate) root: Arc<crate::rooted::Root>,
    /// Root-relative descendant path.
    pub(crate) relative_path: PathBuf,
}
