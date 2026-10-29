// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Debug, Eq, PartialEq)]
/// Windows filesystem name and volume attribute flags.
///
/// Attribute flags are preserved as reported by the platform; observing them
/// does not alter volume attributes such as `READONLY`.
pub struct LocalWindowsFileSystemObservation {
    filesystem_name: String,
    volume_attributes: u32,
}
impl LocalWindowsFileSystemObservation {
    #[cfg(windows)]
    #[allow(dead_code)] // Consumed when T04 connects Windows security queries.
    pub(crate) fn new(filesystem_name: String, volume_attributes: u32) -> Self {
        Self {
            filesystem_name,
            volume_attributes,
        }
    }
    /// Returns the filesystem name reported by Windows.
    pub fn filesystem_name(&self) -> &str {
        &self.filesystem_name
    }
    /// Returns the native volume attribute bitmask.
    pub const fn volume_attributes(&self) -> u32 {
        self.volume_attributes
    }
}

#[cfg(test)]
mod tests {
    use super::LocalWindowsFileSystemObservation;
    use crate::observation::LocalFileSystemLocality;
    use crate::observation::LocalFileSystemObservation;

    #[test]
    fn test_windows_filesystem_facts_preserve_name_and_unknown_attribute_bits() {
        let windows = LocalWindowsFileSystemObservation {
            filesystem_name: "ReFS".to_owned(),
            volume_attributes: 0x8000_0001,
        };
        let facts = LocalFileSystemObservation::new(LocalFileSystemLocality::Unknown, None, Some(windows));
        assert_eq!(facts.locality(), LocalFileSystemLocality::Unknown);
        assert!(facts.unix().is_none());
        let windows = facts.windows().expect("Windows filesystem facts");
        assert_eq!(windows.filesystem_name(), "ReFS");
        assert_eq!(windows.volume_attributes(), 0x8000_0001);
    }
}
