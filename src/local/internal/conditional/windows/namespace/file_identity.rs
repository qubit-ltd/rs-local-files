// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable identity data used to bind Windows namespace entries to open handles.

/// Identifies a filesystem object by its volume and file identifier.
///
/// This value is used internally to verify that a reopened namespace entry
/// still refers to the object observed through a retained handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::local::internal::conditional::windows) struct FileIdentity {
    /// Serial number of the volume containing the identified file.
    pub(super) volume_serial: u64,
    /// Filesystem-provided identifier for the file on that volume.
    pub(super) file_id: [u8; 16],
}
