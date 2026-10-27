//! Same-handle file observations and source verification values.

mod types;
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;

pub use types::{
    LocalFileIdentity, LocalFileObservation, LocalNativeFileDetails, LocalObservationRequirement,
    LocalSourceVerification, LocalUnixFileDetails, LocalWindowsFileDetails,
};
