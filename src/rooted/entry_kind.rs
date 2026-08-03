// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative filesystem entry kinds.
// qubit-style: allow source-test-pair

/// The type of a rooted filesystem entry observed without following its final
/// symbolic link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
    /// A named pipe (FIFO).
    Fifo,
    /// A Unix-domain or platform-specific socket.
    Socket,
    /// A block device.
    BlockDevice,
    /// A character device.
    CharDevice,
    /// A platform-specific special entry.
    Other,
}
