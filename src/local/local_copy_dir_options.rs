/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
/// Options controlling recursive directory copy behavior.
///
/// The default is conservative: existing destination entries are not
/// overwritten, symbolic links are not followed, and source permissions are not
/// copied to destination entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCopyDirOptions {
    /// Whether existing destination files may be overwritten.
    pub overwrite: bool,

    /// Whether symbolic links in the source tree should be followed.
    ///
    /// When this is `false`, encountering a symbolic link returns
    /// [`std::io::ErrorKind::Unsupported`]. This avoids accidentally copying
    /// data outside the requested source tree.
    pub follow_symlinks: bool,

    /// Whether to copy source permissions to destination entries after copying.
    ///
    /// This uses `std::fs::set_permissions` and therefore only preserves the
    /// portable permission bits exposed by the Rust standard library.
    pub preserve_permissions: bool,
}

impl Default for LocalCopyDirOptions {
    /// Returns conservative directory copy options.
    ///
    /// # Returns
    /// Options that do not overwrite existing destination entries, do not
    /// follow symbolic links, and do not preserve source permissions.
    #[inline]
    fn default() -> Self {
        Self {
            overwrite: false,
            follow_symlinks: false,
            preserve_permissions: false,
        }
    }
}
