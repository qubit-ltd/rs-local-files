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
