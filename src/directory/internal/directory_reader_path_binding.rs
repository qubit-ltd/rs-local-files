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
    /// Formats the binding while redacting its filesystem paths.
    ///
    /// Only the stable type name and read options are exposed; path recipes and
    /// request coordinates can contain sensitive filesystem locations.
    ///
    /// # Parameters
    ///
    /// `formatter` receives the redacted debug representation.
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
    ///
    /// # Parameters
    ///
    /// `filesystem`, `component_policy`, and `authority_deadline` preserve the
    /// fixed directory's access boundary. `original` is the caller's unexpanded
    /// recipe; `request_path` is the error-reporting coordinate. `options` and
    /// `identity` capture the reader's verification contract and observed file.
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
    /// Returns the earliest deadline imposed by read options or directory
    /// authority.
    ///
    /// # Returns
    ///
    /// Returns `Some` with the earliest configured deadline, or `None` when
    /// neither source imposes a deadline.
    fn verification_deadline(&self) -> Option<Duration> {
        self.options
            .path_binding()
            .and_then(|options| options.deadline())
            .into_iter()
            .chain(self.authority_deadline)
            .min()
    }

    /// Reopens the original recipe and compares its identity with the captured
    /// identity.
    ///
    /// A missing target is reported as `Missing`; other I/O failures are
    /// preserved. An expired deadline returns a timed-out metadata error at
    /// the request path.
    ///
    /// # Parameters
    ///
    /// `deadline` is an optional caller-supplied verification deadline.
    ///
    /// # Returns
    ///
    /// Returns `Unchanged` when the reopened identity matches, `Changed` when
    /// it differs, or `Missing` when the target no longer exists. Other
    /// failures, including an expired deadline, are returned as errors.
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
