// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by public capability integration tests.

use super::LocalPathLimit;

/// Immutable snapshot of native filesystem guarantees available to this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemCapabilities {
    /// Known native path limit.
    path_limit: Option<LocalPathLimit>,
    /// Whether descriptor- or handle-relative rooted operations are available.
    rooted_operations: bool,
    /// Whether an in-filesystem rename is atomic.
    atomic_rename: bool,
    /// Whether replacing a destination is atomic.
    atomic_replace: bool,
    /// Whether a temporary resource can be persisted without replacement.
    atomic_temp_persist: bool,
    /// Whether directory durability synchronization is implemented.
    directory_durability: bool,
}

impl LocalFileSystemCapabilities {
    /// Detects capabilities for the current build and runtime platform.
    #[inline]
    pub(crate) const fn detect() -> Self {
        Self {
            // PATH_MAX is a process-header bound, not a verified limit of the
            // target filesystem. This crate does not currently probe the
            // mounted filesystem, so report the limit as unknown.
            path_limit: None,
            rooted_operations: cfg!(any(unix, windows)),
            atomic_rename: cfg!(any(unix, windows)),
            atomic_replace: cfg!(any(unix, windows)),
            atomic_temp_persist: cfg!(any(
                target_os = "linux",
                target_os = "macos",
                windows
            )),
            directory_durability: cfg!(unix),
        }
    }

    /// Returns the native path limit, or `None` when no stable limit is known.
    #[must_use]
    #[inline(always)]
    pub const fn path_limit(self) -> Option<LocalPathLimit> {
        self.path_limit
    }

    /// Reports whether secure rooted operations are available.
    #[must_use]
    #[inline(always)]
    pub const fn supports_rooted_operations(self) -> bool {
        self.rooted_operations
    }

    /// Reports whether an in-filesystem rename is atomic.
    #[must_use]
    #[inline(always)]
    pub const fn supports_atomic_rename(self) -> bool {
        self.atomic_rename
    }

    /// Reports whether replacing an existing destination is atomic.
    #[must_use]
    #[inline(always)]
    pub const fn supports_atomic_replace(self) -> bool {
        self.atomic_replace
    }

    /// Reports whether temporary-resource persistence without replacement is
    /// atomic.
    #[must_use]
    #[inline(always)]
    pub const fn supports_atomic_temp_persist(self) -> bool {
        self.atomic_temp_persist
    }

    /// Reports whether parent-directory durability synchronization is
    /// implemented.
    #[must_use]
    #[inline(always)]
    pub const fn supports_directory_durability(self) -> bool {
        self.directory_durability
    }
}
