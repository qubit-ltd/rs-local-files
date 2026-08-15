// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Compile-selected native filesystem primitives.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(crate) use unix::DirectoryCursor;
#[cfg(unix)]
pub(crate) use unix::EntryIdentity;
#[cfg(unix)]
pub(crate) use unix::NamespaceHandle;
#[cfg(unix)]
pub(crate) use unix::OpenedFile;
#[cfg(unix)]
pub(crate) use unix::StagedFile;
#[cfg(windows)]
pub(crate) use windows::DirectoryCursor;
#[cfg(windows)]
pub(crate) use windows::EntryIdentity;
#[cfg(windows)]
pub(crate) use windows::NamespaceHandle;
#[cfg(windows)]
pub(crate) use windows::OpenedFile;
#[cfg(windows)]
pub(crate) use windows::PlatformDirectoryEntry;
#[cfg(windows)]
pub(crate) use windows::StagedFile;
#[cfg(windows)]
pub(crate) use windows::StagedInstallError;
#[cfg(windows)]
pub(crate) use windows::StagedInstallState;

#[cfg(test)]
mod platform_tests;
