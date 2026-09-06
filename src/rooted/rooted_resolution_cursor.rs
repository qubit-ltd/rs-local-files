// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
// =============================================================================
//! Component-at-a-time observation for rooted path resolution.

use std::ffi::OsStr;
use std::fs::File;
#[cfg(not(any(unix, windows)))]
use std::io::Error;
#[cfg(not(any(unix, windows)))]
use std::io::ErrorKind;
use std::io::Result;

use super::Metadata;

/// A temporary directory authority used while checking a rooted path.
///
/// Only the current directory is retained. A successful descent replaces the
/// previous handle, so the number of live descriptors stays constant with
/// path depth.
pub(crate) struct RootedResolutionCursor {
    current: File,
    #[cfg(test)]
    observation: std::cell::Cell<ResolutionObservation>,
}

impl RootedResolutionCursor {
    /// Creates a cursor from an already-opened rooted directory authority.
    pub(crate) fn new(current: File) -> Result<Self> {
        Ok(Self {
            current,
            #[cfg(test)]
            observation: std::cell::Cell::new(ResolutionObservation::default()),
        })
    }

    /// Reads one child without following a final symbolic link.
    pub(crate) fn metadata(&self, name: &OsStr) -> Result<Metadata> {
        #[cfg(test)]
        self.observation.update_metadata();
        #[cfg(unix)]
        {
            let status = crate::local::read_rooted_component_metadata(&self.current, name)?;
            Ok(Metadata::from_stat(&status))
        }
        #[cfg(windows)]
        {
            let file = crate::local::read_rooted_component_metadata(&self.current, name)?;
            Metadata::from_open_file(&file)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = name;
            Err(unsupported_resolution_error())
        }
    }

    /// Descends into one child after verifying that it is a real directory.
    ///
    /// The current handle is left unchanged if opening or verification fails.
    pub(crate) fn descend(&mut self, name: &OsStr) -> Result<()> {
        #[cfg(test)]
        self.observation.update_directory_open();
        #[cfg(any(unix, windows))]
        {
            let next = crate::local::open_rooted_component_directory(&self.current, name)?;
            self.current = next;
            Ok(())
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = name;
            Err(unsupported_resolution_error())
        }
    }

    /// Returns the private primitive-call observations used by complexity
    /// regression tests.
    #[cfg(test)]
    fn observation(&self) -> ResolutionObservation {
        self.observation.get()
    }
}

/// Returns the stable unsupported-platform error for rooted cursors.
#[cfg(not(any(unix, windows)))]
fn unsupported_resolution_error() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "descriptor-relative rooted resolution is unsupported on this platform",
    )
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ResolutionObservation {
    metadata_calls: usize,
    directory_opens: usize,
}

#[cfg(test)]
trait ObservationCellExt {
    fn update_metadata(&self);
    fn update_directory_open(&self);
}

#[cfg(test)]
impl ObservationCellExt for std::cell::Cell<ResolutionObservation> {
    fn update_metadata(&self) {
        let mut observation = self.get();
        observation.metadata_calls += 1;
        self.set(observation);
    }

    fn update_directory_open(&self) {
        let mut observation = self.get();
        observation.directory_opens += 1;
        self.set(observation);
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs;

    use super::RootedResolutionCursor;
    use crate::rooted::Root;

    #[test]
    fn cursor_observes_components_linearly_and_releases_previous_directory() {
        let temporary = tempfile::tempdir().expect("temporary directory should be created");
        fs::create_dir_all(temporary.path().join("a/b/c/d")).expect("fixture should be created");
        fs::write(temporary.path().join("a/b/c/d/file"), b"payload").expect("file should be created");
        let root = Root::open(temporary.path()).expect("root should open");
        let mut cursor = RootedResolutionCursor::new(root.try_clone_authority().expect("root should clone"))
            .expect("cursor should open");

        for component in ["a", "b", "c", "d"] {
            assert_eq!(
                crate::rooted::EntryKind::Directory,
                cursor
                    .metadata(OsStr::new(component))
                    .expect("directory metadata should read")
                    .kind(),
            );
            cursor.descend(OsStr::new(component)).expect("directory should open");
        }
        assert_eq!(
            crate::rooted::EntryKind::File,
            cursor
                .metadata(OsStr::new("file"))
                .expect("file metadata should read")
                .kind(),
        );
        let observation = cursor.observation();
        assert_eq!(5, observation.metadata_calls);
        assert_eq!(4, observation.directory_opens);
    }

    #[test]
    fn failed_descent_keeps_the_current_directory_authority() {
        let temporary = tempfile::tempdir().expect("temporary directory should be created");
        fs::create_dir(temporary.path().join("child")).expect("fixture should be created");
        fs::write(temporary.path().join("file"), b"payload").expect("file should be created");
        let root = Root::open(temporary.path()).expect("root should open");
        let mut cursor = RootedResolutionCursor::new(root.try_clone_authority().expect("root should clone"))
            .expect("cursor should open");
        assert!(cursor.descend(OsStr::new("file")).is_err());
        assert_eq!(
            crate::rooted::EntryKind::Directory,
            cursor
                .metadata(OsStr::new("child"))
                .expect("current root should remain usable")
                .kind(),
        );
    }
}
