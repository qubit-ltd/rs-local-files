//! Same-handle file observations and source verification values.

mod types;
pub(crate) mod snapshot;
pub(crate) mod query;
pub(crate) mod reader;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub use types::{
    LocalFileIdentity, LocalFileObservation, LocalNativeFileDetails, LocalObservationRequirement,
    LocalSourceVerification, LocalUnixFileDetails, LocalWindowsFileDetails,
};
pub use reader::LocalObservedReader;
pub use snapshot::LocalFileSnapshot;
