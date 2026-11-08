// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by structured error integration tests.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::string::FromUtf8Error;

use super::LocalFileEffectState;
use super::LocalFileErrorKind;
use super::LocalFileErrorSource;
use super::LocalFileOperation;
use super::LocalPathCodecError;
use super::LocalResourceLimitError;
use crate::LocalTempRecovery;

/// Structured failure from a local filesystem operation.
///
/// Some partial temporary-resource failures attach a
/// [`crate::LocalTempRecovery`] to the error. In that case, the error owns
/// cleanup responsibility: dropping the error also drops the recovery owner and
/// makes its silent best-effort cleanup attempt. Use [`Self::temp_recovery`] to
/// inspect the owner or [`Self::into_temp_recovery_parts`] to transfer it
/// before dropping the error.
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalFileError;
/// use qubit_local_files::error::LocalFileErrorKind;
/// use qubit_local_files::error::LocalFileOperation;
///
/// let error = LocalFileError::new(LocalFileErrorKind::NotFound, LocalFileOperation::Metadata);
/// assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
/// assert_eq!(error.operation(), LocalFileOperation::Metadata);
/// ```
#[must_use]
#[derive(Debug)]
pub struct LocalFileError {
    /// Stable failure classification.
    kind: LocalFileErrorKind,

    /// Operation that failed.
    operation: LocalFileOperation,

    /// Namespace-absolute PWD used to bind relative operation paths.
    current_directory: Option<Box<PathBuf>>,

    /// Primary path in the operation's public namespace.
    path: Option<PathBuf>,

    /// Secondary or destination path in the public namespace.
    target: Option<PathBuf>,

    /// Stable explanation for policy or validation failures.
    reason: Option<&'static str>,

    /// Additional diagnostic detail for failures whose stable reason is shared.
    detail: Option<Box<str>>,

    /// Typed source retained from the originating failure.
    source: Option<LocalFileErrorSource>,

    /// Namespace effect explicitly established by an operation scheduler.
    effect_state: Option<LocalFileEffectState>,

    /// Secondary cleanup failure retained without replacing the primary error.
    cleanup_error: Option<Box<LocalFileError>>,

    /// Private temporary resource retained after a partial creation failure.
    temp_recovery: Option<Box<LocalTempRecovery>>,
}

impl LocalFileError {
    /// Creates a structured error without native I/O context.
    ///
    /// # Parameters
    ///
    /// - `kind`: Stable failure classification.
    /// - `operation`: Operation that failed.
    ///
    /// # Returns
    ///
    /// A structured error containing the supplied classification and
    /// operation, without native I/O context.
    #[inline]
    pub const fn new(kind: LocalFileErrorKind, operation: LocalFileOperation) -> Self {
        Self {
            kind,
            operation,
            current_directory: None,
            path: None,
            target: None,
            reason: None,
            detail: None,
            source: None,
            effect_state: None,
            cleanup_error: None,
            temp_recovery: None,
        }
    }

    /// Converts a native I/O failure and preserves path context.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed.
    /// - `path`: Optional primary public-namespace path.
    /// - `target`: Optional destination public-namespace path.
    /// - `source`: Native I/O error.
    ///
    /// # Returns
    ///
    /// A structured local filesystem error.
    #[inline]
    pub fn from_io(
        operation: LocalFileOperation,
        path: Option<PathBuf>,
        target: Option<PathBuf>,
        source: io::Error,
    ) -> Self {
        if let Some(resource_limit) = source
            .get_ref()
            .and_then(|source| source.downcast_ref::<LocalResourceLimitError>())
            .copied()
        {
            let mut error = Self::from_resource_limit(operation, path, resource_limit);
            error.target = target;
            return error;
        }
        Self {
            kind: classify_io_error(&source),
            operation,
            current_directory: None,
            path,
            target,
            reason: None,
            detail: None,
            source: Some(LocalFileErrorSource::Io(source)),
            effect_state: None,
            cleanup_error: None,
            temp_recovery: None,
        }
    }

    /// Converts a canonical path codec failure and preserves path context.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed while converting a path.
    /// - `path`: Optional primary native path context.
    /// - `error`: Canonical path codec failure to retain as the typed source.
    ///
    /// # Returns
    ///
    /// A structured invalid-path error whose source is `PathCodec(error)`.
    #[allow(dead_code)]
    #[inline]
    pub(crate) fn from_path_codec(
        operation: LocalFileOperation,
        path: Option<PathBuf>,
        error: LocalPathCodecError,
    ) -> Self {
        Self {
            kind: LocalFileErrorKind::InvalidPath,
            operation,
            current_directory: None,
            path,
            target: None,
            reason: None,
            detail: None,
            source: Some(LocalFileErrorSource::PathCodec(error)),
            effect_state: None,
            cleanup_error: None,
            temp_recovery: None,
        }
    }

    /// Converts a local resource budget failure and preserves its facts.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed.
    /// - `path`: Optional primary path.
    /// - `source`: Resource budget facts to retain as the typed source.
    ///
    /// # Returns
    ///
    /// A structured resource-limit error whose source is
    /// `ResourceLimit(source)`.
    #[inline]
    pub(crate) fn from_resource_limit(
        operation: LocalFileOperation,
        path: Option<PathBuf>,
        source: LocalResourceLimitError,
    ) -> Self {
        Self {
            kind: LocalFileErrorKind::ResourceLimit,
            operation,
            current_directory: None,
            path,
            target: None,
            reason: None,
            detail: None,
            source: Some(LocalFileErrorSource::ResourceLimit(source)),
            effect_state: None,
            cleanup_error: None,
            temp_recovery: None,
        }
    }

    /// Creates an invalid-encoding error while retaining the UTF-8 failure.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation that failed while decoding text.
    /// - `path`: Optional primary path associated with the operation.
    /// - `source`: UTF-8 conversion failure retained as the typed source.
    ///
    /// # Returns
    ///
    /// A structured invalid-encoding error retaining `source`.
    #[inline]
    pub(crate) fn from_utf8(operation: LocalFileOperation, path: Option<PathBuf>, source: FromUtf8Error) -> Self {
        Self {
            kind: LocalFileErrorKind::InvalidEncoding,
            operation,
            current_directory: None,
            path,
            target: None,
            reason: None,
            detail: None,
            source: Some(LocalFileErrorSource::InvalidEncoding(source)),
            effect_state: None,
            cleanup_error: None,
            temp_recovery: None,
        }
    }

    /// Returns the best available classification of the underlying cause.
    ///
    /// For ordinary errors this is the stable classification returned by
    /// [`Self::kind`]. Publication and indeterminate failures retain their
    /// outer effect classification in `kind`; when a typed source is present,
    /// this method exposes the source's cause classification separately.
    ///
    /// `None` means that a publication or indeterminate failure was created
    /// without a typed source, so its cause cannot be inferred safely.
    ///
    /// # Returns
    ///
    /// `Some(kind)` contains the source cause, or this error's kind for an
    /// ordinary failure. `None` means the cause cannot be inferred because
    /// this is a publication or indeterminate failure without a typed source.
    #[inline]
    pub fn cause_kind(&self) -> Option<LocalFileErrorKind> {
        if !matches!(
            self.kind,
            LocalFileErrorKind::PublicationIncomplete | LocalFileErrorKind::Indeterminate
        ) {
            return Some(self.kind);
        }
        match self.source.as_ref() {
            Some(LocalFileErrorSource::InvalidEncoding(_)) => Some(LocalFileErrorKind::InvalidEncoding),
            Some(LocalFileErrorSource::Recovery(error)) => error.cause_kind(),
            Some(LocalFileErrorSource::Io(source)) => Some(classify_io_error(source)),
            Some(LocalFileErrorSource::ResourceLimit(_)) => Some(LocalFileErrorKind::ResourceLimit),
            Some(LocalFileErrorSource::PathCodec(_)) => Some(LocalFileErrorKind::InvalidPath),
            None => None,
        }
    }

    /// Returns the cleanup failure, when cleanup failed after the primary
    /// operation had already produced an error.
    ///
    /// # Returns
    ///
    /// `Some(error)` borrows the retained cleanup failure; `None` means no
    /// cleanup failure was attached.
    #[inline]
    pub fn cleanup_error(&self) -> Option<&LocalFileError> {
        self.cleanup_error.as_deref()
    }

    /// Returns the namespace-absolute PWD retained from relative path binding.
    ///
    /// `None` means no PWD snapshot was attached, including absolute-path
    /// calls.
    ///
    /// # Returns
    ///
    /// `Some(path)` borrows the retained namespace-absolute PWD; `None` means
    /// no snapshot was attached.
    #[inline]
    pub fn current_directory(&self) -> Option<&Path> {
        self.current_directory.as_deref().map(PathBuf::as_path)
    }

    /// Returns the namespace effect state represented by this error.
    ///
    /// Basic errors do not contain enough evidence to infer an effect, so
    /// they return `None`. In particular, `None` does not mean
    /// [`LocalFileEffectState::Unchanged`].
    ///
    /// # Returns
    ///
    /// `Some(state)` contains an explicitly retained state or the state
    /// implied by a publication-incomplete or indeterminate error. `None`
    /// means the error does not establish a namespace effect state.
    #[inline]
    pub fn effect_state(&self) -> Option<LocalFileEffectState> {
        if let Some(effect_state) = self.effect_state {
            return Some(effect_state);
        }
        match self.kind {
            LocalFileErrorKind::PublicationIncomplete => Some(LocalFileEffectState::PartiallyApplied),
            LocalFileErrorKind::Indeterminate => Some(LocalFileEffectState::Indeterminate),
            _ => None,
        }
    }

    /// Returns the retained native I/O source, when the failure originated in
    /// a standard-library I/O operation.
    ///
    /// # Returns
    ///
    /// `Some(error)` borrows the retained native I/O source, including one
    /// nested in a recovery error; `None` means no native I/O source is
    /// retained.
    #[inline]
    pub fn io_error(&self) -> Option<&io::Error> {
        match self.source.as_ref() {
            Some(LocalFileErrorSource::Io(error)) => Some(error),
            Some(LocalFileErrorSource::Recovery(error)) => error.io_error(),
            _ => None,
        }
    }

    /// Returns the standard I/O kind represented by this structured error.
    ///
    /// # Returns
    ///
    /// The standard I/O kind corresponding to the retained source or stable
    /// local failure classification.
    #[must_use]
    #[inline]
    pub fn io_error_kind(&self) -> io::ErrorKind {
        standard_io_error_kind(self)
    }

    /// Returns the stable failure classification.
    ///
    /// # Returns
    ///
    /// The stable local failure classification.
    #[inline]
    pub const fn kind(&self) -> LocalFileErrorKind {
        self.kind
    }

    /// Returns the operation that failed.
    ///
    /// # Returns
    ///
    /// The local filesystem operation associated with this error.
    #[inline]
    pub const fn operation(&self) -> LocalFileOperation {
        self.operation
    }

    /// Returns the primary path, or `None` when no path applies.
    ///
    /// # Returns
    ///
    /// `Some(path)` borrows the primary public-namespace path; `None` means
    /// this error has no primary path.
    #[inline]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Returns the stable human-readable explanation, when one was provided.
    ///
    /// # Returns
    ///
    /// `Some(reason)` borrows the stable explanation; `None` means no reason
    /// was attached.
    #[inline]
    pub const fn reason(&self) -> Option<&str> {
        self.reason
    }

    /// Returns the retained local resource-limit source, when present.
    ///
    /// # Returns
    ///
    /// `Some(error)` borrows the retained resource-limit source, including one
    /// nested in a recovery error; `None` means no such source is retained.
    #[inline]
    pub fn resource_limit_error(&self) -> Option<&LocalResourceLimitError> {
        match self.source.as_ref() {
            Some(LocalFileErrorSource::ResourceLimit(error)) => Some(error),
            Some(LocalFileErrorSource::Recovery(error)) => error.resource_limit_error(),
            _ => None,
        }
    }

    /// Returns the destination path, or `None` for single-path operations.
    ///
    /// # Returns
    ///
    /// `Some(path)` borrows the destination path; `None` means no destination
    /// path applies.
    #[inline]
    pub fn target(&self) -> Option<&Path> {
        self.target.as_deref()
    }

    /// Returns the private temporary resource retained after a partial
    /// creation failure.
    ///
    /// `Some` borrows the owner held by this error; `None` means no recovery
    /// resource was attached. The error itself owns cleanup responsibility, so
    /// dropping it also drops the owner and attempts cleanup silently. Use
    /// [`Self::into_temp_recovery_parts`] to keep the owner beyond the error's
    /// lifetime.
    ///
    /// # Returns
    ///
    /// `Some(recovery)` borrows the retained recovery owner; `None` means no
    /// recovery resource was attached.
    #[inline]
    pub fn temp_recovery(&self) -> Option<&LocalTempRecovery> {
        self.temp_recovery.as_deref()
    }

    /// Returns the typed source retained from the originating failure.
    ///
    /// # Returns
    ///
    /// `Some` contains an I/O, UTF-8, path codec, resource-limit, or recovery
    /// source; `None` means this error was constructed without an originating
    /// source or retained recovery resource.
    #[inline]
    pub const fn typed_source(&self) -> Option<&LocalFileErrorSource> {
        self.source.as_ref()
    }

    /// Appends a cleanup failure to the end of the retained cleanup chain.
    ///
    /// The primary error and any existing cleanup chain remain unchanged. The
    /// supplied error, including its own nested cleanup chain, is appended at
    /// the end. Each error keeps ownership of any [`crate::LocalTempRecovery`]
    /// attached to it, so dropping an error in the chain also drops its
    /// recovery owner and makes that owner's silent best-effort cleanup
    /// attempt. Use [`Self::temp_recovery`] to inspect an owner or
    /// [`Self::into_temp_recovery_parts`] to transfer it before the error is
    /// dropped.
    ///
    /// # Parameters
    ///
    /// - `cleanup`: Cleanup failure to append after all previously retained
    ///   cleanup failures.
    ///
    /// # Returns
    ///
    /// The updated primary error.
    ///
    /// # Examples
    ///
    /// ```
    /// use qubit_local_files::error::LocalFileError;
    /// use qubit_local_files::error::LocalFileErrorKind;
    /// use qubit_local_files::error::LocalFileOperation;
    ///
    /// let make = |reason| LocalFileError::new(
    ///     LocalFileErrorKind::Io,
    ///     LocalFileOperation::Read,
    /// ).with_reason(reason);
    /// let error = make("primary")
    ///     .with_additional_cleanup_error(make("first"))
    ///     .with_additional_cleanup_error(make("second"));
    /// let first = error.cleanup_error().expect("first cleanup error");
    /// assert_eq!(first.reason(), Some("first"));
    /// let second = first.cleanup_error().expect("second cleanup error");
    /// assert_eq!(second.reason(), Some("second"));
    /// ```
    #[inline]
    pub fn with_additional_cleanup_error(mut self, cleanup: LocalFileError) -> Self {
        let mut slot = &mut self.cleanup_error;
        while let Some(existing) = slot {
            slot = &mut existing.cleanup_error;
        }
        *slot = Some(Box::new(cleanup));
        self
    }

    /// Retains a cleanup failure without replacing this primary failure.
    ///
    /// This replaces any cleanup error already attached to this error. The
    /// replaced cleanup chain is dropped, including any recovery owners it
    /// contains; dropping those owners makes their silent best-effort cleanup
    /// attempt. Use [`Self::with_additional_cleanup_error`] to preserve the
    /// existing chain and append another cleanup failure.
    ///
    /// # Parameters
    ///
    /// - `cleanup`: Cleanup failure to retain, replacing any previously
    ///   attached cleanup chain.
    ///
    /// # Returns
    ///
    /// The updated primary error.
    #[inline]
    pub fn with_cleanup_error(mut self, cleanup: LocalFileError) -> Self {
        self.cleanup_error = Some(Box::new(cleanup));
        self
    }

    /// Adds the namespace-absolute PWD used for path binding.
    ///
    /// # Parameters
    ///
    /// - `current_directory`: Namespace-absolute PWD snapshot used to resolve
    ///   relative paths.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub fn with_current_directory(mut self, current_directory: PathBuf) -> Self {
        self.current_directory = Some(Box::new(current_directory));
        self
    }

    /// Adds a primary path to this error.
    ///
    /// # Parameters
    ///
    /// - `path`: Public-namespace path that was being accessed.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    /// Adds a stable explanation for the failure.
    ///
    /// # Parameters
    ///
    /// - `reason`: Static human-readable explanation of the failed requirement
    ///   or validation rule.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub const fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    /// Adds a destination path to this error.
    ///
    /// # Parameters
    ///
    /// - `target`: Public-namespace destination path.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub fn with_target(mut self, target: PathBuf) -> Self {
        self.target = Some(target);
        self
    }

    /// Consumes this structured error as a standard I/O error.
    ///
    /// # Returns
    ///
    /// An I/O error that preserves the originating native kind when available
    /// and retains this structured error as its source.
    #[must_use]
    #[inline]
    pub fn into_io_error(self) -> io::Error {
        io::Error::new(self.io_error_kind(), self)
    }

    /// Consumes the error and returns its typed source, if present.
    ///
    /// # Returns
    ///
    /// `Some` contains an I/O, UTF-8, path codec, resource-limit, or recovery
    /// source; `None` means there is no originating source or retained recovery
    /// resource.
    /// If this error owns a temporary recovery resource, the returned
    /// [`LocalFileErrorSource::Recovery`] retains the whole error, including
    /// that cleanup responsibility. Dropping the returned source eventually
    /// drops the recovery owner and attempts cleanup silently.
    #[inline]
    pub fn into_source(mut self) -> Option<LocalFileErrorSource> {
        if self.temp_recovery.is_some() {
            Some(LocalFileErrorSource::Recovery(Box::new(self)))
        } else {
            self.source.take()
        }
    }

    /// Consumes this error and transfers its retained recovery resource.
    ///
    /// The returned `Some` owner carries the cleanup responsibility formerly
    /// held by this error. If it is dropped, it attempts cleanup silently; keep
    /// the owner if you need to inspect or retry cleanup. The returned error
    /// preserves the primary failure and no longer owns the recovery resource.
    /// `None` means this error had no retained recovery resource.
    ///
    /// # Returns
    ///
    /// The error with its recovery owner transferred, and `Some(owner)` when
    /// one was retained; otherwise the second tuple element is `None`.
    #[must_use = "the error and transferred recovery owner should be handled"]
    #[inline]
    pub fn into_temp_recovery_parts(mut self) -> (LocalFileError, Option<LocalTempRecovery>) {
        let recovery = self.temp_recovery.take().map(|recovery| *recovery);
        (self, recovery)
    }

    /// Replaces public namespace path context during facade remapping.
    ///
    /// # Parameters
    ///
    /// - `path`: New primary path, or `None` when no primary path applies.
    /// - `target`: New destination path, or `None` when no destination applies.
    /// - `current_directory`: New PWD snapshot, or `None` when no snapshot
    ///   applies.
    pub(crate) fn replace_paths(
        &mut self,
        path: Option<PathBuf>,
        target: Option<PathBuf>,
        current_directory: Option<PathBuf>,
    ) {
        self.path = path;
        self.target = target;
        self.current_directory = current_directory.map(Box::new);
    }

    /// Adds dynamic diagnostic detail while preserving the stable error
    /// classification and reason. The detail is retained as an owned string
    /// and included in the displayed error.
    ///
    /// # Parameters
    ///
    /// - `detail`: Additional human-readable context for this failure.
    ///
    /// # Returns
    ///
    /// The updated error.
    pub(crate) fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail.into_boxed_str());
        self
    }

    /// Retains namespace effect facts established by an operation scheduler.
    ///
    /// # Parameters
    ///
    /// - `effect_state`: Effect state established by the scheduler.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub(crate) const fn with_effect_state(mut self, effect_state: LocalFileEffectState) -> Self {
        self.effect_state = Some(effect_state);
        self
    }

    /// Reclassifies an error while retaining its native source and paths.
    ///
    /// # Parameters
    ///
    /// - `kind`: More precise classification established by the caller.
    ///
    /// # Returns
    ///
    /// The reclassified error.
    #[inline]
    pub(crate) const fn with_kind(mut self, kind: LocalFileErrorKind) -> Self {
        self.kind = kind;
        self
    }

    /// Reclassifies the operation after a shared validation stage.
    ///
    /// # Parameters
    ///
    /// - `operation`: Operation to associate with the remapped failure.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub(crate) const fn with_operation(mut self, operation: LocalFileOperation) -> Self {
        self.operation = operation;
        self
    }

    /// Adds ownership of a partially created private temporary resource.
    ///
    /// # Parameters
    ///
    /// - `recovery`: Owner of the partial resource and its cleanup
    ///   responsibility.
    ///
    /// # Returns
    ///
    /// The updated error.
    #[inline]
    pub(crate) fn with_temp_recovery(mut self, recovery: LocalTempRecovery) -> Self {
        self.temp_recovery = Some(Box::new(recovery));
        self
    }
}

/// Selects the standard I/O kind to expose when adapting this error.
///
/// Native I/O sources retain their exact kind. Errors without one use the
/// closest stable local classification.
///
/// # Parameters
///
/// - `error`: Structured error whose retained source and classification are
///   used to select the standard I/O kind.
///
/// # Returns
///
/// The closest standard I/O kind for this error.
#[must_use]
#[inline]
fn standard_io_error_kind(error: &LocalFileError) -> io::ErrorKind {
    match error.source.as_ref() {
        Some(LocalFileErrorSource::Io(source)) => source.kind(),
        Some(LocalFileErrorSource::InvalidEncoding(_)) => io::ErrorKind::InvalidData,
        Some(LocalFileErrorSource::Recovery(error)) => error.io_error_kind(),
        Some(LocalFileErrorSource::PathCodec(_)) => io::ErrorKind::InvalidInput,
        Some(LocalFileErrorSource::ResourceLimit(_)) => io::ErrorKind::Other,
        None => match error.kind {
            LocalFileErrorKind::AlreadyExists => io::ErrorKind::AlreadyExists,
            LocalFileErrorKind::InvalidPath | LocalFileErrorKind::InvalidOptions | LocalFileErrorKind::InvalidState => {
                io::ErrorKind::InvalidInput
            }
            LocalFileErrorKind::NotDirectory => io::ErrorKind::NotADirectory,
            LocalFileErrorKind::IsDirectory => io::ErrorKind::IsADirectory,
            LocalFileErrorKind::NotFound => io::ErrorKind::NotFound,
            LocalFileErrorKind::PermissionDenied => io::ErrorKind::PermissionDenied,
            LocalFileErrorKind::ResourceLimit => io::ErrorKind::Other,
            LocalFileErrorKind::DataCorruption => io::ErrorKind::InvalidData,
            LocalFileErrorKind::InvalidEncoding => io::ErrorKind::InvalidData,
            LocalFileErrorKind::RequirementNotMet | LocalFileErrorKind::Unsupported => io::ErrorKind::Unsupported,
            _ => io::ErrorKind::Other,
        },
    }
}

impl fmt::Display for LocalFileError {
    /// Formats the structured operation and available native path context.
    ///
    /// # Parameters
    ///
    /// - `formatter`: Destination formatter supplied by the formatting
    ///   framework.
    ///
    /// # Returns
    ///
    /// `Ok(())` after writing the error, or the formatter's error if writing
    /// fails.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?} failed with {:?}", self.operation, self.kind)?;
        if let Some(reason) = self.reason {
            write!(formatter, ": {reason}")?;
        }
        if let Some(detail) = &self.detail {
            write!(formatter, " ({detail})")?;
        }
        if let Some(current_directory) = &self.current_directory {
            write!(formatter, " from PWD {}", current_directory.display())?;
        }
        if let Some(path) = &self.path {
            write!(formatter, " at {}", path.display())?;
        }
        if let Some(target) = &self.target {
            write!(formatter, " targeting {}", target.display())?;
        }
        if let Some(source) = &self.source {
            write!(formatter, "; caused by {source}")?;
        }
        Ok(())
    }
}

impl Error for LocalFileError {
    /// Returns the concrete I/O, path codec, or resource-limit source, if
    /// present.
    ///
    /// # Returns
    ///
    /// `Some(source)` borrows the concrete source error; `None` means the
    /// structured error has no exposed source.
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().and_then(Error::source)
    }
}

/// Classifies an operating-system I/O error without discarding its source.
///
/// # Parameters
///
/// - `error`: Native I/O failure to classify.
///
/// # Returns
///
/// The stable local error kind corresponding to the native error.
#[inline]
fn classify_io_error(error: &io::Error) -> LocalFileErrorKind {
    match error.kind() {
        io::ErrorKind::NotFound => LocalFileErrorKind::NotFound,
        io::ErrorKind::AlreadyExists => LocalFileErrorKind::AlreadyExists,
        io::ErrorKind::NotADirectory => LocalFileErrorKind::NotDirectory,
        io::ErrorKind::IsADirectory => LocalFileErrorKind::IsDirectory,
        io::ErrorKind::PermissionDenied => LocalFileErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput => LocalFileErrorKind::InvalidPath,
        io::ErrorKind::InvalidData => LocalFileErrorKind::DataCorruption,
        io::ErrorKind::Unsupported => LocalFileErrorKind::Unsupported,
        io::ErrorKind::OutOfMemory | io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => {
            LocalFileErrorKind::ResourceLimit
        }
        _ => LocalFileErrorKind::Io,
    }
}
