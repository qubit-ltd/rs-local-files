// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Exact owner permissions to apply and verify on one supported platform.
///
/// Unix modes and Windows access masks are separate platform configurations;
/// there are no implicit permission presets. The builder stores values and
/// validation is delayed until a consuming owner-policy operation. Invalid
/// values are rejected as `InvalidOptions` before native objects are mutated.
/// Exact owner-policy operations are supported on Linux and Windows; macOS
/// reports `Unsupported` because it cannot provide the required verification
/// contract.
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::{LocalDirectoryOpenOptions, LocalListOptions, LocalTempDirectoryOptions};
/// use qubit_local_files::policy::LocalOwnerAccessPolicy;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let parent = filesystem.open_directory_with_options(
///     fixture.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// #[cfg(target_os = "linux")]
/// let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
/// #[cfg(windows)]
/// let policy = LocalOwnerAccessPolicy::new()
///     .with_windows_masks(0x0016_01ef, 0x0016_019f);
/// #[cfg(any(target_os = "linux", windows))]
/// {
///     let options = LocalTempDirectoryOptions::new().with_owner_access_policy(policy);
///     let mut stage = match parent.create_staged_directory(&options) {
///         Ok(stage) => stage,
///         Err(error)
///             if error.cause().kind() == qubit_local_files::error::LocalFileErrorKind::PermissionDenied =>
///         {
///             return Ok(());
///         }
///         Err(error) => return Err(error.into()),
///     };
///     if let Err(error) = stage.apply_owner_access_policy_with_options(&policy, &LocalListOptions::new()) {
///         if error.cause().kind() == qubit_local_files::error::LocalFileErrorKind::PermissionDenied {
///             return Ok(());
///         }
///         return Err(error.into());
///     }
/// }
/// #[cfg(target_os = "macos")]
/// {
///     // Exact owner-policy application is unsupported on macOS.
///     let _ = LocalOwnerAccessPolicy::new();
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
pub struct LocalOwnerAccessPolicy {
    unix_modes: Option<(u32, u32)>,
    windows_masks: Option<(u32, u32)>,
}
impl LocalOwnerAccessPolicy {
    /// Creates an unconfigured policy without implicit permission presets.
    pub const fn new() -> Self {
        Self {
            unix_modes: None,
            windows_masks: None,
        }
    }
    /// Sets exact Unix directory and file modes; only owner `rwx` bits are
    /// valid.
    pub const fn with_unix_modes(mut self, directory_mode: u32, file_mode: u32) -> Self {
        self.unix_modes = Some((directory_mode, file_mode));
        self
    }
    /// Sets exact Windows directory and file access masks.
    ///
    /// Each mask may contain only the FILE-specific rights bits `0x0000_01ff`
    /// and standard object rights `DELETE` (`0x0001_0000`), `READ_CONTROL`
    /// (`0x0002_0000`), `WRITE_DAC` (`0x0004_0000`), `WRITE_OWNER`
    /// (`0x0008_0000`), and `SYNCHRONIZE` (`0x0010_0000`), whose union is
    /// `0x001f_01ff`. `GENERIC_*`, `MAXIMUM_ALLOWED`,
    /// `ACCESS_SYSTEM_SECURITY`, and all unknown bits are rejected.
    ///
    /// This builder stores the requested values without validating them.
    /// The consuming owner-policy operation validates the masks before it
    /// mutates any native object and reports invalid values as
    /// [`crate::error::LocalFileErrorKind::InvalidOptions`].
    pub const fn with_windows_masks(mut self, directory_mask: u32, file_mask: u32) -> Self {
        self.windows_masks = Some((directory_mask, file_mask));
        self
    }
    #[cfg(unix)]
    pub(crate) const fn unix_modes(&self) -> Option<(u32, u32)> {
        self.unix_modes
    }
    #[cfg(windows)]
    pub(crate) const fn windows_masks(&self) -> Option<(u32, u32)> {
        self.windows_masks
    }
}
