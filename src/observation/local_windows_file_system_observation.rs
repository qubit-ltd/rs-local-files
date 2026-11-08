// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Windows filesystem name and volume attribute flags.
///
/// Attribute flags are preserved as reported by the platform; observing them
/// does not alter volume attributes such as `READONLY`.
///
/// # Examples
///
/// A value is provided by a Windows filesystem observation. A caller can
/// inspect the value through the public API:
///
/// ```
/// use qubit_local_files::observation::LocalWindowsFileSystemObservation;
///
/// fn inspect_filesystem(observation: &LocalWindowsFileSystemObservation) {
///     let filesystem_name = observation.filesystem_name();
///     assert!(!filesystem_name.is_empty());
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWindowsFileSystemObservation {
    /// Filesystem name reported by the Windows volume query.
    filesystem_name: String,
    /// Native Windows volume attribute bitmask, preserved without filtering.
    volume_attributes: u32,
}
impl LocalWindowsFileSystemObservation {
    /// Creates filesystem facts from the Windows volume query results.
    ///
    /// The query backend supplies the filesystem name and native attribute
    /// bits; this constructor does not interpret or modify those bits.
    ///
    /// # Parameters
    ///
    /// * `filesystem_name` - The name returned by the Windows volume query.
    /// * `volume_attributes` - The native attribute bitmask returned by the
    ///   query, preserved without filtering.
    #[cfg(windows)]
    #[allow(dead_code)] // Consumed when T04 connects Windows security queries.
    #[must_use]
    pub(crate) fn new(filesystem_name: String, volume_attributes: u32) -> Self {
        Self {
            filesystem_name,
            volume_attributes,
        }
    }
    /// Returns the filesystem name reported by Windows.
    #[must_use]
    #[inline]
    pub fn filesystem_name(&self) -> &str {
        &self.filesystem_name
    }
    /// Returns the native volume attribute bitmask.
    #[must_use]
    #[inline]
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
