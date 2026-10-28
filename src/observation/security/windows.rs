use crate::{
    LocalResult,
    observation::{LocalSecurityObservation, LocalSecurityObservationOptions},
};
use std::fs::File;

pub(crate) fn query_security(
    _file: &File,
    _options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    Err(crate::error::LocalFileError::new(
        crate::error::LocalFileErrorKind::Unsupported,
        crate::error::LocalFileOperation::Metadata,
    ))
}
