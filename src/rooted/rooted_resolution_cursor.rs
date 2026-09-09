// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
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
    /// The sole owned directory authority, replaced only after successful
    /// descent.
    current: File,
}

impl RootedResolutionCursor {
    /// Creates a cursor from an already-opened rooted directory authority.
    #[inline(always)]
    pub(crate) fn new(current: File) -> Result<Self> {
        Ok(Self { current })
    }

    /// Reads one child without following a final symbolic link.
    pub(crate) fn metadata(&self, name: &OsStr) -> Result<Metadata> {
        #[cfg(test)]
        crate::tests::rooted::support::resolution_observation::record_metadata();
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
        crate::tests::rooted::support::resolution_observation::record_directory_open();
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
}

/// Returns the stable unsupported-platform error for rooted cursors.
#[cfg(not(any(unix, windows)))]
fn unsupported_resolution_error() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "descriptor-relative rooted resolution is unsupported on this platform",
    )
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use std::ffi::OsStr;
    use std::fs;
    #[cfg(unix)]
    use std::os::fd::AsRawFd;
    #[cfg(windows)]
    use std::os::windows::io::AsRawHandle;

    use super::RootedResolutionCursor;
    use crate::rooted::EntryKind;
    use crate::rooted::Root;

    /// Runs alone so another test cannot reuse a just-closed native handle.
    #[test]
    fn test_cursor_releases_replaced_authority_and_retains_failed_descent() {
        const CHILD: &str = "QUBIT_CURSOR_HANDLE_LIFECYCLE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let status = std::process::Command::new(std::env::current_exe().expect("test executable should exist"))
                .args(["--exact", "rooted::rooted_resolution_cursor::tests::test_cursor_releases_replaced_authority_and_retains_failed_descent", "--test-threads=1"])
                .env(CHILD, "1")
                .status().expect("isolated lifecycle test should launch");
            assert!(status.success());
            return;
        }
        let temporary = tempfile::tempdir().expect("temporary directory should exist");
        fs::create_dir(temporary.path().join("child")).expect("child should exist");
        fs::write(temporary.path().join("child/file"), b"payload").expect("file should exist");
        let root = Root::open(temporary.path()).expect("root should open");
        let mut cursor = RootedResolutionCursor::new(root.try_clone_authority().expect("authority should clone"))
            .expect("cursor should open");
        #[cfg(unix)]
        let old = cursor.current.as_raw_fd();
        #[cfg(windows)]
        let old = cursor.current.as_raw_handle();
        cursor
            .descend(OsStr::new("child"))
            .expect("directory descent should succeed");
        #[cfg(unix)]
        {
            // SAFETY: F_GETFD only inspects the integer descriptor; it does not
            // dereference memory or take ownership of a possibly closed file.
            assert_eq!(-1, unsafe { libc::fcntl(old, libc::F_GETFD) });
            assert_eq!(Some(libc::EBADF), std::io::Error::last_os_error().raw_os_error());
        }
        #[cfg(windows)]
        {
            let mut flags = 0;
            // SAFETY: The output pointer is valid; querying a closed handle
            // returns an error and never transfers ownership.
            assert_eq!(0, unsafe {
                windows_sys::Win32::Foundation::GetHandleInformation(old, &mut flags)
            });
        }
        #[cfg(unix)]
        let retained = cursor.current.as_raw_fd();
        #[cfg(windows)]
        let retained = cursor.current.as_raw_handle();
        assert!(cursor.descend(OsStr::new("file")).is_err());
        #[cfg(unix)]
        assert_eq!(retained, cursor.current.as_raw_fd());
        #[cfg(windows)]
        assert_eq!(retained, cursor.current.as_raw_handle());
        assert_eq!(
            EntryKind::File,
            cursor
                .metadata(OsStr::new("file"))
                .expect("retained authority must remain usable")
                .kind()
        );
    }
}
