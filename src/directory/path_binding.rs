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

/// Retains the name and identity needed to verify an observed file binding.
///
/// Verification reopens the captured path through the same filesystem policy
/// and reports whether it still resolves to the original object. The binding
/// keeps the reader options and limits used for that resolution.
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
    /// Formats the binding while redacting its captured path.
    ///
    /// # Parameters
    ///
    /// * `formatter` — destination that receives the redacted debug fields.
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
    ///
    /// # Parameters
    ///
    /// * `filesystem` — filesystem namespace used for later path resolution.
    /// * `path` — namespace-absolute path to resolve again.
    /// * `identity` — identity observed when the reader was opened.
    /// * `observed_options` — reader options to reuse during resolution.
    /// * `options` — path-binding limits and deadline policy.
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

    /// Reopens the captured name and compares its identity with the original.
    ///
    /// # Parameters
    ///
    /// * `deadline` — optional cooperative deadline for this verification.
    ///
    /// # Returns
    ///
    /// `Unchanged` if the original identity is still resolved, `Changed` if a
    /// different identity is resolved, or `Missing` if the name no longer
    /// exists. Other resolution failures are returned as errors.
    ///
    /// # Errors
    ///
    /// Returns an error if path resolution fails for a reason other than the
    /// captured name being absent, or if the verification deadline expires.
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
    fn test_path_binding_reopen_obeys_verification_deadline() {
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
