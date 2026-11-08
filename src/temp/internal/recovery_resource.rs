// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::PathBuf;

use super::native_private_entry::NativePrivateEntry;
use crate::temp::LocalTempFile;

/// Resource ownership held by temporary-resource recovery.
///
/// `LiveFile` delegates cleanup to the ordinary temporary-file owner.
/// `Native` retains native handles and names after partial creation or owner
/// construction failure. Moving a variant transfers cleanup responsibility to
/// the recovery object; it does not imply cleanup has already succeeded.
#[derive(Debug)]
pub(crate) enum RecoveryResource {
    /// Delegates cleanup to an already constructed temporary-file owner.
    ///
    /// Moving the owner transfers cleanup responsibility to this recovery
    /// resource; the owner retains its authority and identity checks.
    ///
    /// The boxed owner avoids embedding the larger temporary-file value in
    /// this enum and is moved back out when recovery is converted to a file.
    LiveFile(
        /// The owner whose retained authority and identity govern cleanup.
        Box<LocalTempFile>,
    ),
    /// Retains native entries created before a file owner could be completed.
    ///
    /// Entries are cleaned in vector order. Each path is diagnostic context;
    /// cleanup authority comes from the retained native handles and identities.
    Native(
        /// Diagnostic paths paired with entries that retain cleanup authority.
        Vec<(PathBuf, NativePrivateEntry)>,
    ),
}
