// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::LocalFileSystem;
use crate::LocalResult;
use crate::directory::LocalComponentPolicy;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;
use crate::options::LocalObservedReadOptions;

/// Retains a fixed directory's original relative recipe for name verification.
pub(crate) struct DirectoryReaderPathBinding {
    /// Filesystem clone retaining the same directory authority.
    filesystem: LocalFileSystem,
    /// Unexpanded relative path supplied by the caller.
    original: PathBuf,
    /// Namespace coordinate used for operation errors during verification.
    request_path: PathBuf,
    /// Component policy inherited from the fixed directory handle.
    component_policy: LocalComponentPolicy,
    /// Read and binding contracts captured when the reader was opened.
    options: LocalObservedReadOptions,
    /// Identity observed from the initially opened reader.
    identity: crate::observation::LocalFileIdentity,
    /// Deadline inherited from the fixed directory handle.
    authority_deadline: Option<Duration>,
}

impl std::fmt::Debug for DirectoryReaderPathBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DirectoryReaderPathBinding")
            .field("original", &crate::local_file_system::redacted_path(&self.original))
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl DirectoryReaderPathBinding {
    /// Captures the fixed authority and original path recipe.
    pub(crate) fn new(
        filesystem: LocalFileSystem,
        original: PathBuf,
        request_path: PathBuf,
        component_policy: LocalComponentPolicy,
        options: LocalObservedReadOptions,
        identity: crate::observation::LocalFileIdentity,
        authority_deadline: Option<Duration>,
    ) -> Self {
        Self {
            filesystem,
            original,
            request_path,
            component_policy,
            options,
            identity,
            authority_deadline,
        }
    }
}

impl ReaderPathBinding for DirectoryReaderPathBinding {
    fn verification_deadline(&self) -> Option<Duration> {
        self.options
            .path_binding()
            .and_then(|options| options.deadline())
            .into_iter()
            .chain(self.authority_deadline)
            .min()
    }

    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        let result = self.filesystem.open_observed_reader_fixed_with_deadline(
            &self.original,
            &self.request_path,
            &self.component_policy,
            &self.options,
            deadline,
            self.authority_deadline,
        );
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Metadata,
                Some(self.request_path.clone()),
                None,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
            ));
        }
        match result {
            Ok(reader) => Ok(if reader.observation().identity() == Some(&self.identity) {
                LocalSourceVerification::Unchanged
            } else {
                LocalSourceVerification::Changed
            }),
            Err(error) if error.kind() == crate::LocalFileErrorKind::NotFound => Ok(LocalSourceVerification::Missing),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;
    use std::time::Instant;

    use super::DirectoryReaderPathBinding;
    use crate::LocalFileSystem;
    use crate::directory::LocalComponentPolicy;
    use crate::observation::LocalObservationRequirement;
    use crate::observation::LocalSourceVerification;
    use crate::observation::reader::ReaderPathBinding;
    use crate::options::LocalObservedReadOptions;

    #[test]
    fn fixed_reader_binding_reopens_its_recipe_and_formats_without_exposing_paths() {
        let fixture = tempfile::tempdir().expect("fixture should be created");
        std::fs::write(fixture.path().join("payload"), b"data").expect("payload should be written");
        let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
        let options = LocalObservedReadOptions::new().with_native_observation(LocalObservationRequirement::Required);
        let reader = filesystem
            .open_observed_reader(Path::new("/payload"), &options)
            .expect("observed reader should open");
        let identity = reader
            .observation()
            .identity()
            .cloned()
            .expect("required observation should include identity");
        let request_path = fixture.path().join("payload");
        let binding = DirectoryReaderPathBinding::new(
            filesystem.clone(),
            Path::new("payload").to_path_buf(),
            request_path.clone(),
            LocalComponentPolicy::allow_all(),
            options,
            identity,
            None,
        );

        assert_eq!(None, binding.verification_deadline());
        assert_eq!(LocalSourceVerification::Unchanged, binding.verify_until(None).unwrap());
        filesystem
            .open_observed_reader_fixed_with_deadline(
                Path::new("payload"),
                &request_path,
                &LocalComponentPolicy::allow_all(),
                &options,
                None,
                None,
            )
            .expect("fixed reader should open with an explicit verification deadline");
        let debug = format!("{binding:?}");
        assert!(debug.contains("DirectoryReaderPathBinding"));
        assert!(!debug.contains(fixture.path().to_string_lossy().as_ref()));

        let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        let error = binding.verify_until(Some(expired)).unwrap_err();
        assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
        assert_eq!(Some(request_path.as_path()), error.path());
    }
}
