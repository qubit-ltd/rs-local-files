// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Normalized kind of a native filesystem entry.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum LocalFileKind {
    /// Regular file.
    File,
    /// Directory.
    Directory,
    /// Symbolic link or Windows name-surrogate reparse point.
    Symlink,
    /// Named pipe (FIFO).
    Fifo,
    /// Unix-domain or platform-specific socket.
    Socket,
    /// Block device.
    BlockDevice,
    /// Character device.
    CharDevice,
    /// Another platform-specific entry that has no stable classification.
    Other,
}
