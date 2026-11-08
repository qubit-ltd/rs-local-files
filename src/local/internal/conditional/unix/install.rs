// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native Unix publication operations.

use std::ffi::CString;
use std::io;
use std::os::fd::AsRawFd;

use super::namespace::Namespace;
use crate::local::LocalAtomicDestinationState;
use crate::local::internal::AtomicStagingState;
use crate::local::internal::atomic_file_install::install_new_atomic_file_at;

/// Atomically exchanges two entries relative to the retained parent directory.
///
/// Both names must be live single components. Linux and macOS provide the
/// exchange operation; other Unix targets report it as unsupported.
///
/// # Parameters
///
/// * `namespace` - Retained directory authority used for both entries.
/// * `left` - Name of the first entry to exchange.
/// * `right` - Name of the second entry to exchange.
///
/// # Returns
///
/// Returns `Ok(())` after the exchange, or the operating-system error if it
/// fails or the target does not support atomic exchange.
///
/// # Errors
///
/// Returns the last operating-system error from the native exchange operation,
/// or `Unsupported` on Unix targets without a supported exchange syscall.
pub(super) fn exchange(namespace: &Namespace, left: &CString, right: &CString) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: both names are live single components and the retained parent
        // descriptor remains open throughout this non-retaining syscall.
        let result = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                namespace.parent.as_raw_fd(),
                left.as_ptr(),
                namespace.parent.as_raw_fd(),
                right.as_ptr(),
                libc::RENAME_EXCHANGE,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: both names and the retained parent descriptor remain live for
        // this atomic same-directory exchange.
        let result = unsafe {
            libc::renameatx_np(
                namespace.parent.as_raw_fd(),
                left.as_ptr(),
                namespace.parent.as_raw_fd(),
                right.as_ptr(),
                libc::RENAME_SWAP,
            )
        };
        if result == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (namespace, left, right);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "atomic conditional exchange is unsupported on this Unix target",
        ))
    }
}

/// Installs the staged entry only if the destination name is still absent.
///
/// Installation is relative to the retained parent directory and never
/// replaces an existing destination. On failure, the destination state from
/// the lower-level installer is translated to the conditional publication
/// state; the staging state is intentionally not returned from this helper.
///
/// # Parameters
///
/// * `namespace` - Retained directory authority and destination name.
/// * `stage` - Single-component name of the staged entry in that directory.
///
/// # Returns
///
/// Returns `Ok(())` when the staged entry is installed, or the error together
/// with the known conditional publication state when installation fails.
///
/// # Errors
///
/// The returned error is the operating-system or installer error. Its paired
/// publication state distinguishes an unchanged/absent destination from a
/// completed or indeterminate publication.
pub(super) fn install_absent(
    namespace: &Namespace,
    stage: &CString,
) -> Result<(), (io::Error, crate::outcome::LocalConditionalPublicationState)> {
    match install_new_atomic_file_at(
        namespace.parent.as_raw_fd(),
        stage,
        namespace.parent.as_raw_fd(),
        &namespace.target_name,
    ) {
        Ok(()) => Ok(()),
        Err((error, destination, staging)) => {
            let publication = match destination {
                LocalAtomicDestinationState::Replaced => crate::outcome::LocalConditionalPublicationState::Published,
                LocalAtomicDestinationState::Indeterminate => {
                    crate::outcome::LocalConditionalPublicationState::Indeterminate
                }
                LocalAtomicDestinationState::Unchanged | LocalAtomicDestinationState::Missing => {
                    crate::outcome::LocalConditionalPublicationState::NotPublished
                }
            };
            let _staging_may_remain = staging != AtomicStagingState::Present;
            Err((error, publication))
        }
    }
}
