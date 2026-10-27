//! Same-handle file observations and source verification values.

pub(crate) mod query;
pub(crate) mod reader;
pub(crate) mod snapshot;
mod types;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub use reader::LocalObservedReader;
pub use snapshot::LocalFileSnapshot;
pub use types::LocalFileIdentity;
pub use types::LocalFileObservation;
pub use types::LocalNativeFileDetails;
pub use types::LocalObservationRequirement;
pub use types::LocalSourceVerification;
pub use types::LocalUnixFileDetails;
pub use types::LocalWindowsFileDetails;
