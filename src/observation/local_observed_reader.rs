// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Same-handle observed readers and bounded collection.

use std::io;
use std::path::PathBuf;

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

const BLOCK_SIZE: usize = 8 * 1024;

/// Reads the complete input subject to an optional byte maximum.
///
/// At the maximum, performs one one-byte probe to distinguish EOF from growth.
/// An input error or allocation failure returns an error and discards the
/// collected vector; bytes already consumed from the input cannot be restored.
pub(crate) fn collect_limited<S: qubit_io::ByteInput + ?Sized>(
    input: &mut S,
    maximum: Option<u64>,
) -> LocalResult<Vec<u8>> {
    collect_limited_with_reserver(input, maximum, |output, additional| {
        output.try_reserve(additional).map_err(io::Error::other)
    })
}

fn collect_limited_with_reserver<S, F>(input: &mut S, maximum: Option<u64>, mut reserve: F) -> LocalResult<Vec<u8>>
where
    S: qubit_io::ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    let mut output = Vec::new();
    let mut count = 0_u64;
    loop {
        let (request, sentinel) = match maximum {
            Some(limit) if count == limit => (1, true),
            Some(limit) => {
                let remaining = limit - count;
                let available = usize::try_from(remaining).unwrap_or(usize::MAX);
                (BLOCK_SIZE.min(available.max(1)), false)
            }
            None => (BLOCK_SIZE, false),
        };
        if sentinel {
            let mut byte = [0_u8; 1];
            match qubit_io::Input::read(input, &mut byte) {
                Ok(0) => return Ok(output),
                Ok(_) => return Err(resource_limit()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(read_error(error)),
            }
        }
        let new_len = output.len().checked_add(request).ok_or_else(resource_limit)?;
        let additional = new_len.saturating_sub(output.len());
        reserve(&mut output, additional).map_err(|_| resource_limit())?;
        let start = output.len();
        output.resize(new_len, 0);
        let read = loop {
            match qubit_io::Input::read(input, &mut output[start..]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        match read {
            Ok(0) => {
                output.truncate(start);
                return Ok(output);
            }
            Ok(read) => {
                if read > request {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::DataCorruption,
                        LocalFileOperation::Read,
                    ));
                }
                output.truncate(start + read);
                count = count
                    .checked_add(u64::try_from(read).map_err(|_| resource_limit())?)
                    .ok_or_else(resource_limit)?;
                if maximum.is_some_and(|limit| count > limit) {
                    return Err(resource_limit());
                }
            }
            Err(error) => return Err(read_error(error)),
        }
    }
}

#[cfg(test)]
pub(crate) fn collect_limited_with_test_reserver<S, F>(
    input: &mut S,
    maximum: Option<u64>,
    reserve: F,
) -> LocalResult<Vec<u8>>
where
    S: qubit_io::ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    collect_limited_with_reserver(input, maximum, reserve)
}

fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

fn read_error(error: io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::Read, None, None, error)
}

pub(crate) trait ReaderPathBinding: std::fmt::Debug + Send + Sync {
    fn verification_deadline(&self) -> Option<std::time::Duration>;

    fn verify_until(&self, deadline: Option<std::time::Instant>) -> LocalResult<LocalSourceVerification>;

    fn verify(&self) -> LocalResult<LocalSourceVerification> {
        let deadline = self
            .verification_deadline()
            .and_then(|limit| std::time::Instant::now().checked_add(limit));
        self.verify_until(deadline)
    }
}

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
/// use qubit_local_files::observation::LocalSourceVerification;
/// use qubit_local_files::options::{LocalObservedReadOptions, LocalPathBindingOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempfile::tempdir()?;
/// std::fs::write(directory.path().join("source"), b"contents")?;
/// let filesystem = LocalFileSystem::rooted(directory.path())?;
/// let binding = LocalPathBindingOptions::new().with_max_ancestors(8);
/// let options = LocalObservedReadOptions::new().with_path_binding(binding);
/// let mut reader = filesystem.open_observed_reader(Path::new("source"), &options)?;
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
    pub(crate) fn from_reader(reader: LocalFileReader, requirement: LocalObservationRequirement) -> LocalResult<Self> {
        let observation = super::query::query_observation(reader.as_file(), requirement)?;
        Ok(Self {
            reader,
            observation,
            requirement,
            binding: None,
            error_path: None,
            error_current_directory: None,
        })
    }

    /// Binds diagnostic coordinates captured by the opening facade.
    pub(crate) fn bind_error_context(mut self, path: PathBuf, current_directory: Option<PathBuf>) -> Self {
        self.error_path = Some(path);
        self.error_current_directory = current_directory;
        self
    }

    /// Adds captured diagnostic coordinates only when the error lacks them.
    ///
    /// Existing path, working-directory, operation, source, and other error
    /// state are retained unchanged.
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

    pub(crate) fn with_binding(mut self, binding: Box<dyn ReaderPathBinding>) -> Self {
        self.binding = Some(match self.binding.take() {
            Some(existing) => Box::new(CombinedPathBinding {
                bindings: vec![existing, binding],
            }),
            None => binding,
        });
        self
    }

    /// Returns the observation captured when this reader was constructed.
    pub fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }

    /// Queries the retained file handle again using the initial requirement.
    ///
    /// This does not change the value returned by [`Self::observation`].
    pub fn observe_now(&self) -> LocalResult<LocalFileObservation> {
        super::query::query_observation(self.reader.as_file(), self.requirement)
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
    /// # Errors
    /// Returns `RequirementNotMet` when a required fact cannot be supplied,
    /// and propagates permission, malformed-data, identity-change, and I/O
    /// failures from the retained handle.
    pub fn security_observation(
        &self,
        options: &LocalSecurityObservationOptions,
    ) -> LocalResult<LocalSecurityObservation> {
        super::security::query_security(self.reader.as_file(), options).map_err(|error| self.contextualize_error(error))
    }

    /// Checks the optional path binding retained when opening this reader.
    ///
    /// Returns `InvalidState` when no binding was requested. Native query
    /// failures are propagated; changed or missing names are reported as a
    /// successful [`LocalSourceVerification`] value.
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
}

#[derive(Debug)]
struct CombinedPathBinding {
    bindings: Vec<Box<dyn ReaderPathBinding>>,
}

impl ReaderPathBinding for CombinedPathBinding {
    fn verification_deadline(&self) -> Option<std::time::Duration> {
        self.bindings
            .iter()
            .filter_map(|binding| binding.verification_deadline())
            .min()
    }

    fn verify_until(&self, deadline: Option<std::time::Instant>) -> LocalResult<LocalSourceVerification> {
        let mut missing = false;
        for binding in &self.bindings {
            match binding.verify_until(deadline)? {
                LocalSourceVerification::Changed => return Ok(LocalSourceVerification::Changed),
                LocalSourceVerification::Missing => missing = true,
                LocalSourceVerification::Unchanged => {}
            }
        }
        Ok(if missing {
            LocalSourceVerification::Missing
        } else {
            LocalSourceVerification::Unchanged
        })
    }
}

impl qubit_io::Input for LocalObservedReader {
    type Item = u8;

    fn read_vectored(&mut self, buffers: &mut [&mut [u8]]) -> io::Result<usize> {
        qubit_io::Input::read_vectored(&mut self.reader, buffers)
    }

    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        // The Input caller guarantees this range; LocalFileReader applies the same
        // contract.
        unsafe { qubit_io::Input::read_unchecked(&mut self.reader, output, index, count) }
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
    }

    impl ReaderPathBinding for DeadlineRecorder {
        fn verification_deadline(&self) -> Option<Duration> {
            Some(Duration::from_secs(1))
        }

        fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
            *self.seen.lock().expect("deadline mutex should be available") = deadline;
            Ok(LocalSourceVerification::Unchanged)
        }
    }

    #[test]
    fn combined_bindings_share_one_verification_deadline() {
        let first = Arc::new(Mutex::new(None));
        let second = Arc::new(Mutex::new(None));
        let binding = CombinedPathBinding {
            bindings: vec![
                Box::new(DeadlineRecorder { seen: first.clone() }),
                Box::new(DeadlineRecorder { seen: second.clone() }),
            ],
        };

        binding.verify().expect("combined binding should verify");

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
