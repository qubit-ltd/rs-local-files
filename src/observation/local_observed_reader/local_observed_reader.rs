// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! The observed reader type and its implementations.

use std::io;
use std::path::PathBuf;

use qubit_io::Input;

use super::ReaderPathBinding;
use super::collect_limited;
use super::combined_path_binding::CombinedPathBinding;
use crate::LocalFileReader;
use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::observation::LocalFileObservation;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;
use crate::observation::LocalSourceVerification;

/// A byte reader whose initial and refreshed observations share its open
/// handle.
///
/// Structured errors may include the path and working directory captured when
/// the reader was opened. These coordinates can become stale after a rename;
/// they neither lock the path nor grant authority. The `qubit_io::Input`
/// interface continues to return `io::Error` and does not promise these
/// coordinates.
///
/// A path binding can additionally check whether the source name still refers
/// to the same native object. Object identity does not identify a content
/// version; use a content digest or another application-level check when that
/// distinction matters.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalObservedReader;
/// use qubit_local_files::observation::LocalSourceVerification;
/// use qubit_local_files::options::LocalObservedReadOptions;
/// use qubit_local_files::options::LocalPathBindingOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempfile::tempdir()?;
/// std::fs::write(directory.path().join("source"), b"contents")?;
/// let filesystem = LocalFileSystem::rooted(directory.path())?;
/// let binding = LocalPathBindingOptions::new().with_max_ancestors(8);
/// let options = LocalObservedReadOptions::new().with_path_binding(binding);
/// let mut reader: LocalObservedReader =
///     filesystem.open_observed_reader(Path::new("source"), &options)?;
/// let bytes = reader.read_to_end_limited(1024)?;
/// assert_eq!(bytes, b"contents");
/// match reader.verify_path_binding()? {
///     LocalSourceVerification::Unchanged => {}
///     LocalSourceVerification::Changed | LocalSourceVerification::Missing => {}
///     _ => {}
/// }
/// // Identity checks the native object, not whether its bytes changed in place.
/// # Ok(())
/// # }
/// ```
pub struct LocalObservedReader {
    /// Open file handle used for reads and same-handle observations.
    reader: LocalFileReader,
    /// Observation captured when this reader was created.
    observation: LocalFileObservation,
    /// Requirement used for subsequent observations.
    requirement: LocalObservationRequirement,
    /// Optional pathname binding used by `verify_path_binding`.
    binding: Option<Box<dyn ReaderPathBinding>>,
    /// Path coordinate captured for structured error diagnostics.
    error_path: Option<PathBuf>,
    /// Working directory captured for structured error diagnostics.
    error_current_directory: Option<PathBuf>,
}

impl std::fmt::Debug for LocalObservedReader {
    /// Formats observation state without exposing the retained file handle.
    ///
    /// # Parameters
    /// - `formatter`: Formatter receiving the reader's diagnostic
    ///   representation.
    ///
    /// # Returns
    /// The formatter's result.
    ///
    /// # Errors
    /// Returns formatting errors reported by the formatter.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalObservedReader")
            .field("observation", &self.observation)
            .field("requirement", &self.requirement)
            .field("binding_enabled", &self.binding.is_some())
            .field("binding", &self.binding)
            .finish()
    }
}

impl LocalObservedReader {
    /// Builds an observed reader from an already-open handle and captures its
    /// initial metadata.
    ///
    /// # Parameters
    /// - `reader`: Open handle retained by the observed reader.
    /// - `requirement`: Metadata requirement applied to the initial
    ///   observation.
    ///
    /// # Returns
    /// A reader with its initial observation and no path binding.
    ///
    /// # Errors
    /// Returns the initial observation's structured query error.
    pub(crate) fn from_reader(reader: LocalFileReader, requirement: LocalObservationRequirement) -> LocalResult<Self> {
        let observation = super::super::query::query_observation(reader.as_file(), requirement)?;
        Ok(Self {
            reader,
            observation,
            requirement,
            binding: None,
            error_path: None,
            error_current_directory: None,
        })
    }

    /// Returns the observation captured when this reader was constructed.
    ///
    /// # Returns
    /// The immutable initial observation, borrowed for the lifetime of `self`.
    #[must_use]
    #[inline]
    pub fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }

    /// Queries the retained file handle again using the initial requirement.
    ///
    /// This does not change the value returned by [`Self::observation`].
    ///
    /// # Returns
    /// A fresh observation queried from the retained handle.
    ///
    /// # Errors
    /// Returns a structured error if the retained handle cannot be queried.
    pub fn observe_now(&self) -> LocalResult<LocalFileObservation> {
        super::super::query::query_observation(self.reader.as_file(), self.requirement)
            .map_err(|error| self.contextualize_error(error))
    }

    /// Queries security facts from this reader's retained file handle.
    ///
    /// Each requested field is queried independently, so returned facts may
    /// describe different instants. The result is not an atomic snapshot;
    /// `Unknown` locality does not establish that the filesystem is local.
    /// Reparse-point handling is governed by the resolution policy used to
    /// open the reader, not by this observation.
    ///
    /// # Parameters
    /// - `options`: Security facts and requirements to query from the retained
    ///   handle.
    ///
    /// # Returns
    /// The independently queried security facts that could be supplied.
    ///
    /// # Errors
    /// Returns `RequirementNotMet` when a required fact cannot be supplied,
    /// and propagates permission, malformed-data, identity-change, and I/O
    /// failures from the retained handle.
    pub fn security_observation(
        &self,
        options: &LocalSecurityObservationOptions,
    ) -> LocalResult<LocalSecurityObservation> {
        super::super::security::query_security(self.reader.as_file(), options)
            .map_err(|error| self.contextualize_error(error))
    }

    /// Checks the optional path binding retained when opening this reader.
    ///
    /// Returns `InvalidState` when no binding was requested. Native query
    /// failures are propagated; changed or missing names are reported as a
    /// successful [`LocalSourceVerification`] value.
    ///
    /// # Returns
    /// Whether the bound source is unchanged, changed, or missing.
    ///
    /// # Errors
    /// Returns `InvalidState` if this reader has no path binding and propagates
    /// resolution, policy, timeout, or native query errors from verification.
    pub fn verify_path_binding(&self) -> LocalResult<LocalSourceVerification> {
        let result = (|| {
            self.binding
                .as_ref()
                .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::Metadata))?
                .verify()
        })();
        result.map_err(|error| self.contextualize_error(error))
    }

    /// Reads through EOF, failing if more than `maximum` bytes are available.
    ///
    /// At exactly the limit, one extra byte may be consumed to distinguish
    /// EOF from growth. Read and allocation errors do not return partial data.
    ///
    /// # Parameters
    /// - `maximum`: Maximum number of bytes to return.
    ///
    /// # Returns
    /// The complete byte vector when EOF occurs within the limit.
    ///
    /// # Errors
    /// Returns `ResourceLimit` when input exceeds the limit, or a read error;
    /// partial bytes are discarded although consumed input cannot be restored.
    pub fn read_to_end_limited(&mut self, maximum: u64) -> LocalResult<Vec<u8>> {
        let result = collect_limited(self, Some(maximum));
        result.map_err(|error| self.contextualize_error(error))
    }

    /// Binds diagnostic coordinates captured by the opening facade.
    ///
    /// # Parameters
    /// - `path`: Path coordinate retained for later structured errors.
    /// - `current_directory`: Opening working directory, when available.
    ///
    /// # Returns
    /// The same reader with diagnostic coordinates attached.
    #[must_use = "the reader with diagnostic coordinates should be retained"]
    pub(crate) fn bind_error_context(mut self, path: PathBuf, current_directory: Option<PathBuf>) -> Self {
        self.error_path = Some(path);
        self.error_current_directory = current_directory;
        self
    }

    /// Adds a path verification binding, combining it with any existing
    /// binding.
    ///
    /// # Parameters
    /// - `binding`: Additional binding to verify with the reader's existing
    ///   binding.
    ///
    /// # Returns
    /// The same reader with the additional binding retained.
    #[must_use = "the reader with the path binding should be retained"]
    pub(crate) fn with_binding(mut self, binding: Box<dyn ReaderPathBinding>) -> Self {
        self.binding = Some(match self.binding.take() {
            Some(existing) => Box::new(CombinedPathBinding {
                bindings: vec![existing, binding],
            }),
            None => binding,
        });
        self
    }

    /// Adds captured diagnostic coordinates only when the error lacks them.
    ///
    /// Existing path, working-directory, operation, source, and other error
    /// state are retained unchanged.
    ///
    /// # Parameters
    /// - `error`: Structured error to supplement with missing coordinates.
    ///
    /// # Returns
    /// The error with captured path coordinates added only where absent.
    fn contextualize_error(&self, mut error: LocalFileError) -> LocalFileError {
        if error.path().is_none()
            && let Some(path) = &self.error_path
        {
            error = error.with_path(path.clone());
        }
        if error.current_directory().is_none()
            && let Some(current_directory) = &self.error_current_directory
        {
            error = error.with_current_directory(current_directory.clone());
        }
        error
    }
}

impl Input for LocalObservedReader {
    /// Byte element read from the retained file handle.
    type Item = u8;

    /// Reads from the retained reader using vectored buffers.
    ///
    /// # Parameters
    /// - `buffers`: Mutable output slices filled in order by the input
    ///   implementation.
    ///
    /// # Returns
    /// The number of bytes transferred from a prefix of the buffers. A short
    /// read, including zero, does not prove permanent EOF.
    ///
    /// # Errors
    /// Returns the native read error without structured path context, or
    /// `InvalidData` if the delegated input reports more bytes than requested.
    fn read_vectored(&mut self, buffers: &mut [&mut [u8]]) -> io::Result<usize> {
        Input::read_vectored(&mut self.reader, buffers)
    }

    /// Delegates indexed reading under the caller's range guarantee.
    ///
    /// # Parameters
    /// - `output`: Destination buffer receiving the requested byte range.
    /// - `index`: Starting offset in `output`.
    /// - `count`: Maximum number of bytes to read.
    ///
    /// # Returns
    /// The number of bytes read, possibly fewer than requested. Zero does not
    /// prove permanent EOF.
    ///
    /// # Errors
    /// Returns the native read error without structured path context.
    ///
    /// # Safety
    /// The caller must guarantee that `index..index + count` is a valid range
    /// inside `output` and that the addition does not overflow.
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        // The Input caller guarantees this range; LocalFileReader applies the same
        // contract.
        unsafe { Input::read_unchecked(&mut self.reader, output, index, count) }
    }
}

#[cfg(test)]
mod binding_tests {
    use std::io;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::time::Duration;
    use std::time::Instant;

    use super::CombinedPathBinding;
    use super::LocalObservedReader;
    use super::ReaderPathBinding;
    use crate::LocalFileSystem;
    use crate::LocalResult;
    use crate::error::LocalFileEffectState;
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileErrorSource;
    use crate::error::LocalFileOperation;
    use crate::observation::LocalObservationRequirement;
    use crate::observation::LocalSourceVerification;
    use crate::options::LocalObservedReadOptions;

    #[derive(Debug)]
    struct DeadlineRecorder {
        seen: Arc<Mutex<Option<Instant>>>,
        deadline: Duration,
    }

    impl ReaderPathBinding for DeadlineRecorder {
        fn verification_deadline(&self) -> Option<Duration> {
            Some(self.deadline)
        }

        fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
            *self.seen.lock().expect("deadline mutex should be available") = deadline;
            Ok(LocalSourceVerification::Unchanged)
        }
    }

    #[test]
    fn test_combined_bindings_share_one_verification_deadline() {
        let first = Arc::new(Mutex::new(None));
        let second = Arc::new(Mutex::new(None));
        let binding = CombinedPathBinding {
            bindings: vec![
                Box::new(DeadlineRecorder {
                    seen: first.clone(),
                    deadline: Duration::ZERO,
                }),
                Box::new(DeadlineRecorder {
                    seen: second.clone(),
                    deadline: Duration::from_secs(1),
                }),
            ],
        };

        assert_eq!(binding.verification_deadline(), Some(Duration::ZERO));
        assert_eq!(
            binding.verify().expect("combined binding should verify"),
            LocalSourceVerification::Unchanged
        );

        let first_deadline = *first.lock().expect("deadline mutex should be available");
        let second_deadline = *second.lock().expect("deadline mutex should be available");
        assert!(first_deadline.is_some());
        assert_eq!(first_deadline, second_deadline);
    }

    #[test]
    fn test_observed_reader_context_fills_only_missing_fields() {
        let fixture = tempfile::tempdir().expect("create fixture");
        let source_path = fixture.path().join("item");
        std::fs::write(&source_path, b"payload").expect("write item");
        let reader = LocalFileSystem::host()
            .expect("open Host")
            .open_observed_reader(&source_path, &LocalObservedReadOptions::new())
            .expect("open reader")
            .bind_error_context(source_path.clone(), Some(fixture.path().to_path_buf()));

        let native = io::Error::other("native source");
        let cleanup = LocalFileError::new(LocalFileErrorKind::NotFound, LocalFileOperation::DeleteFile);
        let original = LocalFileError::from_io(
            LocalFileOperation::Read,
            Some(fixture.path().join("more-specific")),
            Some(fixture.path().join("target")),
            native,
        )
        .with_current_directory(fixture.path().join("existing-pwd"))
        .with_reason("preserve this reason")
        .with_effect_state(LocalFileEffectState::PartiallyApplied)
        .with_cleanup_error(cleanup);
        let result = reader.contextualize_error(original);
        assert_eq!(result.operation(), LocalFileOperation::Read);
        assert_eq!(result.path(), Some(fixture.path().join("more-specific").as_path()));
        assert_eq!(result.target(), Some(fixture.path().join("target").as_path()));
        assert_eq!(
            result.current_directory(),
            Some(fixture.path().join("existing-pwd").as_path())
        );
        assert_eq!(result.reason(), Some("preserve this reason"));
        assert_eq!(result.effect_state(), Some(LocalFileEffectState::PartiallyApplied));
        assert_eq!(
            result.cleanup_error().map(LocalFileError::kind),
            Some(LocalFileErrorKind::NotFound)
        );
        assert!(
            matches!(result.typed_source(), Some(LocalFileErrorSource::Io(source)) if source.to_string() == "native source")
        );

        let missing = LocalFileError::from_io(LocalFileOperation::Read, None, None, io::Error::other("native source"));
        let filled = reader.contextualize_error(missing);
        assert_eq!(filled.path(), Some(source_path.as_path()));
        assert_eq!(filled.current_directory(), Some(fixture.path()));

        let unbound = LocalObservedReader::from_reader(reader.reader, LocalObservationRequirement::Required)
            .expect("construct unbound reader");
        let unbound_error = unbound.contextualize_error(LocalFileError::new(
            LocalFileErrorKind::InvalidState,
            LocalFileOperation::Metadata,
        ));
        assert_eq!(unbound_error.path(), None);
        assert_eq!(unbound_error.current_directory(), None);
    }
}
