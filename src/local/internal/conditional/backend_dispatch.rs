// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;

use super::ConditionalBackend;
#[cfg(all(feature = "digest", feature = "test-support"))]
use super::ConditionalHooks;
use crate::options::LocalWriteOptions;

#[cfg(unix)]
pub(crate) enum ConditionalWriterBackend {
    Native(super::unix::UnixConditionalBackend),
}
#[cfg(windows)]
pub(crate) enum ConditionalWriterBackend {
    Native(super::windows::WindowsConditionalBackend),
}

#[cfg(all(feature = "digest", feature = "test-support", any(unix, windows)))]
impl ConditionalWriterBackend {
    pub(crate) fn set_hooks(&mut self, hooks: ConditionalHooks) {
        match self {
            Self::Native(backend) => backend.set_hooks(hooks),
        }
    }
}

#[cfg(unix)]
impl ConditionalWriterBackend {
    pub(crate) fn host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        super::unix::UnixConditionalBackend::open_host(path, options).map(Self::Native)
    }
    pub(crate) fn rooted(
        root: std::fs::File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        super::unix::UnixConditionalBackend::open_rooted(root, relative, namespace_absolute, diagnostic, options)
            .map(Self::Native)
    }
}
#[cfg(windows)]
impl ConditionalWriterBackend {
    pub(crate) fn host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        super::windows::WindowsConditionalBackend::open_host(path, options).map(Self::Native)
    }
    pub(crate) fn rooted(
        root: std::fs::File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        super::windows::WindowsConditionalBackend::open_rooted(root, relative, namespace_absolute, diagnostic, options)
            .map(Self::Native)
    }
}

#[cfg(not(any(unix, windows)))]
impl ConditionalWriterBackend {
    pub(crate) fn host(_path: &Path, _options: &LocalWriteOptions) -> io::Result<Self> {
        Err(unsupported_error())
    }
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

#[cfg(not(any(unix, windows)))]
fn unsupported_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "conditional writes are unsupported on this platform",
    )
}

impl ConditionalWriterBackend {
    #[cfg(all(feature = "digest", feature = "test-support"))]
    fn checkpoint_before_write(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => backend.checkpoint(super::ConditionalPoint::Write),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }

    #[cfg(not(all(feature = "digest", feature = "test-support")))]
    fn checkpoint_before_write(&mut self) -> io::Result<()> {
        Ok(())
    }

    pub(crate) fn abort(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => backend.abort(),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
}
impl qubit_io::Output for ConditionalWriterBackend {
    type Item = u8;
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => qubit_io::Output::write(backend, bytes),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => qubit_io::Output::write_vectored(backend, slices),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => qubit_io::Output::flush(backend),
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        // SAFETY: delegated to the native backend with the caller-provided valid range.
        self.checkpoint_before_write()?;
        match self {
            #[cfg(any(unix, windows))]
            Self::Native(backend) => unsafe { qubit_io::Output::write_unchecked(backend, input, index, count) },
            #[cfg(not(any(unix, windows)))]
            _ => match *self {},
        }
    }
}
