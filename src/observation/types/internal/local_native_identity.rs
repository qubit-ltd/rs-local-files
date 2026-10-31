// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
/// Platform-specific key used only for native identity comparisons.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(in crate::observation::types) enum LocalNativeIdentity {
    /// Unix device and inode pair.
    #[cfg(unix)]
    Unix {
        /// Native device number.
        device: u64,
        /// Native inode number.
        inode: u64,
    },
    /// Windows volume serial and complete file identifier.
    #[cfg(windows)]
    Windows {
        /// Serial number of the containing volume.
        volume_serial: u64,
        /// Native 128-bit file identifier.
        file_id: [u8; 16],
    },
}
