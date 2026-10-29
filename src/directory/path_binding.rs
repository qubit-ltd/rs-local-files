// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
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
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;

pub(crate) struct PathBinding {
    /// Namespace used to resolve the captured source name during verification.
    filesystem: LocalFileSystem,
    /// Namespace-absolute source name, retained with its original resolution
    /// semantics.
    path: PathBuf,
    /// Native identity observed when the reader was opened.
    identity: crate::observation::LocalFileIdentity,
    /// Reader options reused when the name is resolved again.
    observed_options: crate::options::LocalObservedReadOptions,
    /// Budgets and constraints applied during verification.
    options: crate::options::LocalPathBindingOptions,
}

impl std::fmt::Debug for PathBinding {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PathBinding")
            .field("filesystem", &self.filesystem)
            .field("path", &crate::local_file_system::redacted_path(&self.path))
            .field("identity", &self.identity)
            .field("observed_options", &self.observed_options)
            .field("options", &self.options)
            .finish()
    }
}

impl PathBinding {
    /// Captures the inputs needed to re-resolve and compare a source binding.
    pub(crate) fn new(
        filesystem: LocalFileSystem,
        path: PathBuf,
        identity: crate::observation::LocalFileIdentity,
        observed_options: crate::options::LocalObservedReadOptions,
        options: crate::options::LocalPathBindingOptions,
    ) -> Self {
        Self {
            filesystem,
            path,
            identity,
            observed_options,
            options,
        }
    }
}

impl ReaderPathBinding for PathBinding {
    /// Returns the configured cooperative verification duration, if any.
    fn verification_deadline(&self) -> Option<Duration> {
        self.options.deadline()
    }

    /// Reopens the captured name under the original filesystem and compares
    /// the resulting identity with the initially observed identity.
    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        let result = self.filesystem.open_observed_reader_with_binding_deadline(
            &self.path,
            &self.observed_options,
            self.options,
            deadline,
        );
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Metadata,
                None,
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
            Err(error) if error.kind() == crate::error::LocalFileErrorKind::NotFound => {
                Ok(LocalSourceVerification::Missing)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PathBinding;
    use crate::LocalFileSystem;
    use crate::observation::reader::ReaderPathBinding;
    use crate::options::LocalObservedReadOptions;
    use crate::options::LocalPathBindingOptions;

    #[test]
    fn path_binding_reopen_obeys_verification_deadline() {
        let directory = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(directory.path().join("source"), b"source").expect("source should exist");
        let filesystem = LocalFileSystem::rooted(directory.path()).expect("root should open");
        let reader = filesystem
            .open_observed_reader(std::path::Path::new("source"), &LocalObservedReadOptions::new())
            .expect("reader should open");
        let binding = PathBinding::new(
            filesystem,
            std::path::PathBuf::from("source"),
            reader
                .observation()
                .identity()
                .cloned()
                .expect("identity should be available"),
            LocalObservedReadOptions::new(),
            LocalPathBindingOptions::new().with_deadline(std::time::Duration::ZERO),
        );
        let error = binding.verify().unwrap_err();
        assert_eq!(error.io_error().unwrap().kind(), std::io::ErrorKind::TimedOut);
    }
}
