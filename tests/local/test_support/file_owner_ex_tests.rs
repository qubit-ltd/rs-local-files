// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native Linux extended file-owner descriptor.

/// Native owner descriptor accepted by `fcntl(F_SETOWN_EX)`.
#[repr(C)]
pub(super) struct FileOwnerEx {
    /// Kind of owner stored in `pid`.
    pub(super) owner_type: libc::c_int,
    /// Thread or process identifier receiving the lease-break signal.
    pub(super) pid: libc::pid_t,
}
