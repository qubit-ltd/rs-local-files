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

/// Immutable snapshot of filesystem mechanisms implemented by this build.
///
/// These flags describe code paths available for the current target. They do
/// not probe a particular mount and therefore do not promise that every
/// filesystem used at runtime supports the corresponding native operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemCapabilities {
    /// Known native path limit.
    path_limit: Option<LocalPathLimit>,
    /// Whether descriptor- or handle-relative rooted operations are compiled.
    rooted_operations: bool,
    /// Whether native atomic rename support is implemented.
    atomic_rename: bool,
    /// Whether native atomic replacement support is implemented.
    atomic_replace: bool,
    /// Whether native atomic no-replace persistence is implemented.
    atomic_temp_persist: bool,
    /// Whether directory durability synchronization is implemented.
    directory_durability: bool,
}

impl LocalFileSystemCapabilities {
    /// Detects mechanisms compiled for the current target platform.
    #[inline]
    pub(crate) const fn detect_host() -> Self {
        Self {
            // PATH_MAX is a process-header bound, not a verified limit of the
            // target filesystem. This crate does not currently probe the
            // mounted filesystem, so report the limit as unknown.
            path_limit: None,
            rooted_operations: cfg!(any(unix, windows)),
            atomic_rename: cfg!(any(
                target_os = "linux",
                target_os = "macos",
                windows
            )),
            atomic_replace: cfg!(any(unix, windows)),
            atomic_temp_persist: cfg!(any(
                target_os = "linux",
                target_os = "macos",
                windows
            )),
            directory_durability: cfg!(unix),
        }
    }

    /// Detects mechanisms compiled for a rooted authority on this target.
    #[inline]
    pub(crate) const fn detect_rooted() -> Self {
        Self {
            path_limit: None,
            rooted_operations: cfg!(any(unix, windows)),
            atomic_rename: cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
                windows
            )),
            atomic_replace: cfg!(any(unix, windows)),
            atomic_temp_persist: cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
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

    /// Reports whether secure rooted operations are implemented.
    #[must_use]
    #[inline(always)]
    pub const fn rooted_operations_implemented(self) -> bool {
        self.rooted_operations
    }

    /// Reports whether native atomic rename is implemented.
    #[must_use]
    #[inline(always)]
    pub const fn atomic_rename_implemented(self) -> bool {
        self.atomic_rename
    }

    /// Reports whether native atomic replacement is implemented.
    #[must_use]
    #[inline(always)]
    pub const fn atomic_replace_implemented(self) -> bool {
        self.atomic_replace
    }

    /// Reports whether atomic no-replace temporary persistence is implemented.
    #[must_use]
    #[inline(always)]
    pub const fn atomic_temp_persist_implemented(self) -> bool {
        self.atomic_temp_persist
    }

    /// Reports whether parent-directory durability synchronization is
    /// implemented.
    #[must_use]
    #[inline(always)]
    pub const fn directory_durability_implemented(self) -> bool {
        self.directory_durability
    }
}
