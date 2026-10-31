// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::native_private_entry::NativePrivateEntry;

/// Resource ownership held by temporary-resource recovery.
///
/// `LiveFile` delegates cleanup to the ordinary temporary-file owner.
/// `Native` retains native handles and names after partial creation or owner
/// construction failure. Moving a variant transfers cleanup responsibility to
/// the recovery object; it does not imply cleanup has already succeeded.
#[derive(Debug)]
pub(crate) enum RecoveryResource {
    LiveFile(Box<crate::temp::LocalTempFile>),
    Native(Vec<(std::path::PathBuf, NativePrivateEntry)>),
}
