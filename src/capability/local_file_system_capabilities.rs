// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
/// Immutable snapshot of filesystem operation capabilities implemented by this
/// build.
///
/// These flags describe code paths available for the current target. They do
/// not probe a particular mount and therefore do not promise that every
/// filesystem used at runtime supports the corresponding native operation.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
///
/// let filesystem = LocalFileSystem::host()?;
/// let capabilities = filesystem.capabilities();
/// let _supports_atomic_replace = capabilities.supports_atomic_replace();
/// # Ok::<(), qubit_local_files::LocalFileError>(())
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemCapabilities {
    /// Whether descriptor- or handle-relative rooted operations are compiled.
    rooted_operations: bool,
    /// Whether native atomic rename support is implemented.
    atomic_rename: bool,
    /// Whether native atomic replacement support is implemented.
    atomic_replace: bool,
    /// Whether native atomic no-replace persistence can be attempted.
    atomic_temp_persist_attempt: bool,
    /// Whether durable rename publication is implemented.
    durable_rename: bool,
    /// Whether durable file-copy publication is implemented.
    durable_file_copy: bool,
    /// Whether durable writer publication is implemented.
    durable_write: bool,
}

impl LocalFileSystemCapabilities {
    /// Detects capabilities compiled for the current target platform.
    pub(crate) const fn detect_host() -> Self {
        Self {
            rooted_operations: cfg!(any(unix, windows)),
            atomic_rename: cfg!(any(target_os = "linux", target_os = "macos", windows)),
            atomic_replace: cfg!(any(unix, windows)),
            atomic_temp_persist_attempt: cfg!(any(target_os = "linux", target_os = "macos", windows)),
            durable_rename: cfg!(unix),
            durable_file_copy: cfg!(unix),
            durable_write: cfg!(unix),
        }
    }

    /// Detects capabilities compiled for a rooted authority on this target.
    pub(crate) const fn detect_rooted() -> Self {
        Self {
            rooted_operations: cfg!(any(unix, windows)),
            atomic_rename: cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
                windows
            )),
            atomic_replace: cfg!(any(unix, windows)),
            atomic_temp_persist_attempt: cfg!(any(
                target_os = "linux",
                target_os = "android",
                target_os = "macos",
                target_os = "ios",
                windows
            )),
            durable_rename: cfg!(unix),
            durable_file_copy: cfg!(unix),
            durable_write: cfg!(unix),
        }
    }

    /// Reports whether secure rooted operations are implemented.
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    #[must_use]
    pub const fn supports_rooted_operations(self) -> bool {
        self.rooted_operations
    }

    /// Reports whether native atomic rename is implemented.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_atomic_rename(self) -> bool {
        self.atomic_rename
    }

    /// Reports whether native atomic replacement is implemented.
    #[must_use]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_atomic_replace(self) -> bool {
        self.atomic_replace
    }

    /// Reports whether this build can attempt native atomic no-replace
    /// temporary persistence.
    ///
    /// A `true` result describes an available implementation path, not a
    /// runtime guarantee. Atomic persistence still requires a compatible
    /// source and destination filesystem and may be rejected by mount,
    /// permission, sandbox, or platform policy. Callers must inspect the
    /// persistence outcome or error for the actual operation result.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn can_attempt_atomic_temp_persist(self) -> bool {
        self.atomic_temp_persist_attempt
    }

    /// Reports whether this build can attempt atomic temporary persistence.
    ///
    /// Use [`Self::can_attempt_atomic_temp_persist`] to make the conditional
    /// nature of this build-time capability explicit.
    #[deprecated(
        since = "0.3.0",
        note = "use can_attempt_atomic_temp_persist; runtime success is conditional"
    )]
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_atomic_temp_persist(self) -> bool {
        self.can_attempt_atomic_temp_persist()
    }

    /// Reports whether the full durable rename publication protocol is
    /// implemented for this target.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_durable_rename(self) -> bool {
        self.durable_rename
    }

    /// Reports whether the full durable file-copy publication protocol is
    /// implemented for this target.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_durable_file_copy(self) -> bool {
        self.durable_file_copy
    }

    /// Reports whether the full durable writer publication protocol is
    /// implemented for this target.
    #[must_use]
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn supports_durable_write(self) -> bool {
        self.durable_write
    }
}
