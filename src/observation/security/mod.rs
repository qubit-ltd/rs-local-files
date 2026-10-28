use crate::{
    LocalResult,
    observation::{LocalSecurityObservation, LocalSecurityObservationOptions},
};
use std::fs::File;

pub(crate) fn query_security(
    file: &File,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    query_security_with(file, options, |file, options| {
        #[cfg(unix)]
        {
            unix::query_security(file, options)
        }
        #[cfg(windows)]
        {
            windows::query_security(file, options)
        }
        #[cfg(not(any(unix, windows)))]
        {
            if options.filesystem() == crate::observation::LocalObservationRequirement::Required
                || options.access() == crate::observation::LocalObservationRequirement::Required
                || options.link_count() == crate::observation::LocalObservationRequirement::Required
            {
                return Err(crate::error::LocalFileError::new(
                    crate::error::LocalFileErrorKind::RequirementNotMet,
                    crate::error::LocalFileOperation::Metadata,
                ));
            }
            let _ = file;
            Ok(LocalSecurityObservation::new(None, None, None, None))
        }
    })
}

fn query_security_with(
    file: &File,
    options: &LocalSecurityObservationOptions,
    query: impl FnOnce(&File, &LocalSecurityObservationOptions) -> LocalResult<LocalSecurityObservation>,
) -> LocalResult<LocalSecurityObservation> {
    query(file, options)
}

#[cfg(test)]
mod tests {
    use super::query_security_with;
    use crate::error::{LocalFileError, LocalFileErrorKind, LocalFileOperation};
    use crate::observation::LocalSecurityObservationOptions;
    use std::os::fd::AsRawFd;

    #[test]
    fn injected_security_failures_preserve_kind_on_same_file_handle() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("file");
        std::fs::write(&path, b"data").expect("create file");
        let file = std::fs::File::open(&path).expect("open file");
        let handle = file.as_raw_fd();

        for kind in [
            LocalFileErrorKind::PermissionDenied,
            LocalFileErrorKind::DataCorruption,
            LocalFileErrorKind::ChangedDuringRead,
        ] {
            let error = query_security_with(
                &file,
                &LocalSecurityObservationOptions::new(),
                |queried_file, _| {
                    assert_eq!(queried_file.as_raw_fd(), handle);
                    Err(LocalFileError::new(kind, LocalFileOperation::Metadata))
                },
            )
            .expect_err("injected failure should propagate");
            assert_eq!(error.kind(), kind);
            assert_eq!(file.as_raw_fd(), handle);
        }
    }
}
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;
