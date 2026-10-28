use crate::{
    LocalResult,
    observation::{LocalSecurityObservation, LocalSecurityObservationOptions},
};
use std::fs::File;

pub(crate) fn query_security(
    file: &File,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    #[cfg(unix)]
    { unix::query_security(file, options) }
    #[cfg(windows)]
    { windows::query_security(file, options) }
}
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;
