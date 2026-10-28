// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

const REQUIRE_SYMLINKS_ENV: &str = "QUBIT_LOCAL_FILES_REQUIRE_SYMLINKS";

fn permission_denied_requires_failure(required: bool) -> bool {
    required
}

pub(crate) fn create_file_link(target: &std::path::Path, link: &std::path::Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            if permission_denied_requires_failure(std::env::var(REQUIRE_SYMLINKS_ENV).as_deref() == Ok("1")) {
                panic!("required Windows file symlink fixture is unavailable: {error}");
            }
            eprintln!("skipping Windows file symlink scenario: permission denied creating {link:?}: {error}");
            false
        }
        Err(error) => panic!("Windows file symlink fixture failed for {link:?}: {error}"),
    }
}

pub(crate) fn create_directory_link(target: &std::path::Path, link: &std::path::Path) -> bool {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            if permission_denied_requires_failure(std::env::var(REQUIRE_SYMLINKS_ENV).as_deref() == Ok("1")) {
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
    fn required_symlink_permission_denial_is_failure() {
        assert!(permission_denied_requires_failure(true));
    }

    #[test]
    fn optional_symlink_permission_denial_is_skippable() {
        assert!(!permission_denied_requires_failure(false));
    }
}
