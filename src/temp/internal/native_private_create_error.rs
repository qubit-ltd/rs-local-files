// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;

use super::native_private_entry::NativePrivateEntry;

/// Native private creation failure with ownership of any partial resource.
///
/// `source` describes the failed creation step. When the native operation
/// created an entry before reporting failure, `retained` carries the handles
/// and name needed to transfer that entry to recovery; `None` means no partial
/// resource was retained by the creation helper.
#[derive(Debug)]
pub(crate) struct NativePrivateCreateError {
    /// The operating-system failure reported by the unsuccessful creation step.
    pub(crate) source: io::Error,
    /// The partially created entry retained for recovery, if creation succeeded
    /// before a later step failed.
    pub(crate) retained: Option<NativePrivateEntry>,
}
