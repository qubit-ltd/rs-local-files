// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;

use qubit_io::Output;

use super::super::ConditionalBackend;
#[cfg(all(feature = "digest", feature = "test-support"))]
use super::super::ConditionalHooks;
use crate::options::LocalWriteOptions;

/// Dispatches conditional writes to the native backend for the current
/// platform.
pub(crate) enum ConditionalWriterBackend {
    /// Unix implementation of the conditional-write protocol.
    #[cfg(unix)]
    Native(
        /// Retains the platform handle and protocol state used for each write.
        super::super::unix::UnixConditionalBackend,
    ),
    /// Windows implementation of the conditional-write protocol.
    #[cfg(windows)]
    Native(
        /// Retains the platform handle and protocol state used for each write.
        super::super::windows::WindowsConditionalBackend,
    ),
}

#[cfg(all(feature = "digest", feature = "test-support", any(unix, windows)))]
impl ConditionalWriterBackend {
    /// Installs deterministic test hooks on the selected native backend.
    ///
    /// # Parameters
    ///
    /// * `hooks` - Callbacks invoked at configured conditional-write
    ///   checkpoints.
    pub(crate) fn set_hooks(&mut self, hooks: ConditionalHooks) {
        match self {
            Self::Native(backend) => backend.set_hooks(hooks),
        }
    }
}

#[cfg(unix)]
impl ConditionalWriterBackend {
    /// Opens a conditional writer for a host path.
    ///
    /// # Parameters
    ///
    /// * `path` - Host path whose entry will be conditionally replaced.
    /// * `options` - Write policy applied to the conditional operation.
    ///
    /// # Returns
    ///
    /// A writer backed by the current platform's native implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be opened for conditional writing.
    pub(crate) fn host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        super::super::unix::UnixConditionalBackend::open_host(path, options).map(Self::Native)
    }

    /// Opens a conditional writer relative to an already-open root directory.
    ///
    /// # Parameters
    ///
    /// * `root` - Open directory handle consumed to anchor the operation and
    ///   retained by the backend.
    /// * `relative` - Entry path interpreted relative to `root`.
    /// * `namespace_absolute` - Absolute namespace path used for policy checks.
    /// * `diagnostic` - Optional path included in diagnostic errors.
    /// * `options` - Write policy applied to the conditional operation.
    ///
    /// # Returns
    ///
    /// A writer whose filesystem access is anchored to `root`.
    ///
    /// # Errors
    ///
    /// Returns an error if the rooted path cannot be opened for conditional
    /// writing.
    pub(crate) fn rooted(
        root: std::fs::File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        super::super::unix::UnixConditionalBackend::open_rooted(root, relative, namespace_absolute, diagnostic, options)
            .map(Self::Native)
    }
}

#[cfg(windows)]
impl ConditionalWriterBackend {
    /// Opens a conditional writer for a host path.
    ///
    /// # Parameters
    ///
    /// * `path` - Host path whose entry will be conditionally replaced.
    /// * `options` - Write policy applied to the conditional operation.
    ///
    /// # Returns
    ///
    /// A writer backed by the current platform's native implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be opened for conditional writing.
    pub(crate) fn host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        super::super::windows::WindowsConditionalBackend::open_host(path, options).map(Self::Native)
    }

    /// Opens a conditional writer relative to an already-open root directory.
    ///
    /// # Parameters
    ///
    /// * `root` - Open directory handle consumed to anchor the operation and
    ///   retained by the backend.
    /// * `relative` - Entry path interpreted relative to `root`.
    /// * `namespace_absolute` - Absolute namespace path used for policy checks.
    /// * `diagnostic` - Optional path included in diagnostic errors.
    /// * `options` - Write policy applied to the conditional operation.
    ///
    /// # Returns
    ///
    /// A writer whose filesystem access is anchored to `root`.
    ///
    /// # Errors
    ///
    /// Returns an error if the rooted path cannot be opened for conditional
    /// writing.
    pub(crate) fn rooted(
        root: std::fs::File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        super::super::windows::WindowsConditionalBackend::open_rooted(
            root,
            relative,
            namespace_absolute,
            diagnostic,
            options,
        )
        .map(Self::Native)
    }
}

#[cfg(not(any(unix, windows)))]
impl ConditionalWriterBackend {
    /// Returns the platform's unsupported-operation error for a host path.
    ///
    /// # Parameters
    ///
    /// * `_path` - Unsupported host path; it is unused on this platform.
    /// * `_options` - Write policy; it is unused on this platform.
    ///
    /// # Returns
    ///
    /// No writer is produced on unsupported platforms.
    ///
    /// # Errors
    ///
    /// Always returns `ErrorKind::Unsupported`.
    pub(crate) fn host(_path: &Path, _options: &LocalWriteOptions) -> io::Result<Self> {
        Err(unsupported_error())
    }

    /// Returns the platform's unsupported-operation error for a rooted path.
    ///
    /// # Parameters
    ///
    /// * `_root` - Root handle; it is unused and dropped on this platform.
    /// * `_relative` - Relative entry path; it is unused on this platform.
    /// * `_namespace_absolute` - Namespace path; it is unused on this platform.
    /// * `_diagnostic` - Optional diagnostic path; it is unused on this
    ///   platform.
    /// * `_options` - Write policy; it is unused on this platform.
    ///
    /// # Returns
    ///
    /// No writer is produced on unsupported platforms.
    ///
    /// # Errors
    ///
    /// Always returns `ErrorKind::Unsupported`.
    pub(crate) fn rooted(
        _root: std::fs::File,
        _relative: &Path,
        _namespace_absolute: &Path,
        _diagnostic: Option<&Path>,
        _options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        Err(unsupported_error())
    }
}

/// Constructs the shared unsupported-platform error.
///
/// # Returns
///
/// An `Unsupported` error describing the unavailable conditional-write
/// operation.
#[cfg(not(any(unix, windows)))]
fn unsupported_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "conditional writes are unsupported on this platform",
    )
}

impl ConditionalWriterBackend {
    /// Aborts the active native conditional-write operation.
    ///
    /// # Returns
    ///
    /// `Ok(())` after aborting the active operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot complete abort cleanup.
    pub(crate) fn abort(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => backend.abort(),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    /// Runs the configured checkpoint before writing bytes.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the checkpoint permits the write.
    ///
    /// # Errors
    ///
    /// Returns an injected checkpoint error that prevents the write.
    #[cfg(all(feature = "digest", feature = "test-support"))]
    fn checkpoint_before_write(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => backend.checkpoint(super::super::ConditionalPoint::Write),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    /// Runs the no-op checkpoint used when test hooks are disabled.
    ///
    /// # Returns
    ///
    /// Always returns `Ok(())` because no test hook is installed.
    #[cfg(not(all(feature = "digest", feature = "test-support")))]
    fn checkpoint_before_write(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Output for ConditionalWriterBackend {
    /// Byte type accepted by the conditional writer.
    type Item = u8;

    /// Writes bytes after running the configured write checkpoint.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Input bytes offered to the selected backend.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted, which may be less than `bytes.len()`.
    ///
    /// # Errors
    ///
    /// Returns a checkpoint or backend write error.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => Output::write(backend, bytes),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    /// Writes from several byte slices after the configured checkpoint.
    ///
    /// # Parameters
    ///
    /// * `slices` - Ordered input slices offered to the selected backend.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted from the slices; the write may be short.
    ///
    /// # Errors
    ///
    /// Returns a checkpoint or backend write error.
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => Output::write_vectored(backend, slices),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    /// Flushes the selected native backend.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the selected backend flushes its pending bytes.
    ///
    /// # Errors
    ///
    /// Returns the backend flush error.
    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => Output::flush(backend),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    /// Writes a validated range without repeating the caller's bounds checks.
    ///
    /// # Parameters
    ///
    /// * `input` - Source bytes containing the requested range.
    /// * `index` - Start of the range in `input`.
    /// * `count` - Number of bytes requested from the range.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted from the requested range; the write may be
    /// short.
    ///
    /// # Errors
    ///
    /// Returns a checkpoint or backend write error.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `index..index + count` is a valid range in
    /// `input` and that adding `index` and `count` does not overflow, as
    /// required by [`Output::write_unchecked`].
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        // SAFETY: delegated to the native backend with the caller-provided valid range.
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => unsafe { Output::write_unchecked(backend, input, index, count) },
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
}
