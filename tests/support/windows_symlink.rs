// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::env;
use std::io;
use std::path::Path;

const REQUIRE_SYMLINKS_ENV: &str = "QUBIT_LOCAL_FILES_REQUIRE_SYMLINKS";

#[must_use]
#[inline]
fn permission_denied_requires_failure(required: bool) -> bool {
    required
}

#[must_use]
pub(crate) fn create_file_link(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            if permission_denied_requires_failure(env::var(REQUIRE_SYMLINKS_ENV).as_deref() == Ok("1")) {
                panic!("required Windows file symlink fixture is unavailable: {error}");
            }
            eprintln!("skipping Windows file symlink scenario: permission denied creating {link:?}: {error}");
            false
        }
        Err(error) => panic!("Windows file symlink fixture failed for {link:?}: {error}"),
    }
}

#[must_use]
pub(crate) fn create_directory_link(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            if permission_denied_requires_failure(env::var(REQUIRE_SYMLINKS_ENV).as_deref() == Ok("1")) {
                panic!("required Windows directory symlink fixture is unavailable: {error}");
            }
            eprintln!("skipping Windows directory symlink scenario: permission denied creating {link:?}: {error}");
            false
        }
        Err(error) => panic!("Windows directory symlink fixture failed for {link:?}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::permission_denied_requires_failure;

    #[test]
    fn test_required_symlink_permission_denial_fails() {
        assert!(permission_denied_requires_failure(true));
    }

    #[test]
    fn test_optional_symlink_permission_denial_is_skippable() {
        assert!(!permission_denied_requires_failure(false));
    }
}
