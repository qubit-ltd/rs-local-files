// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalUnixPlatform;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Unix filesystem type identifier and optional platform name.
pub struct LocalUnixFileSystemObservation {
    platform: LocalUnixPlatform,
    type_id: u64,
    type_name: Option<String>,
}
impl LocalUnixFileSystemObservation {
    #[cfg(unix)]
    #[allow(dead_code)] // Consumed when T03 connects Unix security queries.
    pub(crate) fn new(platform: LocalUnixPlatform, type_id: u64, type_name: Option<String>) -> Self {
        Self {
            platform,
            type_id,
            type_name,
        }
    }
    /// Returns the Unix platform family.
    pub const fn platform(&self) -> LocalUnixPlatform {
        self.platform
    }
    /// Returns the native filesystem type identifier zero-extended to `u64`.
    pub const fn type_id(&self) -> u64 {
        self.type_id
    }
    /// Returns the platform-provided filesystem type name, or `None` when the
    /// operating system did not provide one.
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }
}
