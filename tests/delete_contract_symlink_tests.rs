// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::options::LocalDeleteOptions;
    use tempfile::tempdir;

    #[test]
    fn final_symlinks_are_deleted_as_entries_and_directory_requests_reject_them() {
        for rooted in [false, true] {
            let directory = tempdir().expect("temporary directory should be created");
            let target = directory.path().join("target");
            let link = directory.path().join("link");
            fs::create_dir(&target).expect("target directory should be created");
            fs::write(target.join("payload"), b"keep").expect("target payload should be written");
            symlink(&target, &link).expect("directory symlink should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("Host filesystem should open")
            };
            let input = if rooted { Path::new("link") } else { link.as_path() };

            let error = filesystem
                .delete_directory_with_options(input, &LocalDeleteOptions::new().with_recursive())
                .expect_err("a final symlink is not a directory entry");
            assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
            assert!(fs::symlink_metadata(&link).is_ok(), "the symlink must survive");
            assert_eq!(fs::read(target.join("payload")).unwrap(), b"keep");

            let outcome = filesystem
                .delete_file_with_options(input, &LocalDeleteOptions::new())
                .expect("a final symlink should be removed as an entry");
            assert!(outcome.deleted());
            assert!(fs::symlink_metadata(&link).is_err(), "the symlink should be removed");
            assert_eq!(fs::read(target.join("payload")).unwrap(), b"keep");
            if !rooted {
                symlink(&target, &link).expect("directory symlink should be restored");
            }
        }
    }

    #[test]
    fn dangling_final_symlinks_are_removed_as_files() {
        for rooted in [false, true] {
            let directory = tempdir().expect("temporary directory should be created");
            let link = directory.path().join("dangling");
            symlink("missing-target", &link).expect("dangling symlink should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("Host filesystem should open")
            };
            let input = if rooted { Path::new("dangling") } else { link.as_path() };

            let outcome = filesystem
                .delete_file_with_options(input, &LocalDeleteOptions::new())
                .expect("a dangling final symlink should be removed");
            assert!(outcome.deleted());
            assert!(fs::symlink_metadata(&link).is_err());
            if !rooted {
                symlink("missing-target", &link).expect("dangling symlink should be restored");
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::fs;
    use std::os::windows::fs::symlink_dir;
    use std::os::windows::fs::symlink_file;
    use std::path::Path;

    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::options::LocalDeleteOptions;
    use tempfile::tempdir;

    #[test]
    fn final_directory_symlinks_are_not_followed_by_delete_contract() {
        let directory = tempdir().expect("temporary directory should be created");
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::create_dir(&target).expect("target directory should be created");
        symlink_dir(&target, &link).expect("directory symlink should be created");
        for rooted in [false, true] {
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("Host filesystem should open")
            };
            let input = if rooted { Path::new("link") } else { link.as_path() };
            let error = filesystem
                .delete_directory_with_options(input, &LocalDeleteOptions::new().with_recursive())
                .expect_err("a final symlink is not a directory entry");
            assert_eq!(LocalFileErrorKind::NotDirectory, error.kind());
            assert!(fs::symlink_metadata(&link).is_ok());
            let outcome = filesystem
                .delete_file_with_options(input, &LocalDeleteOptions::new())
                .expect("a final symlink should be removed as an entry");
            assert!(outcome.deleted());
            symlink_dir(&target, &link).expect("directory symlink should be restored");
        }
    }

    #[test]
    fn final_file_symlinks_are_removed_as_files() {
        let directory = tempdir().expect("temporary directory should be created");
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::write(&target, b"keep").expect("target file should be written");
        symlink_file(&target, &link).expect("file symlink should be created");
        for rooted in [false, true] {
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("Host filesystem should open")
            };
            let input = if rooted { Path::new("link") } else { link.as_path() };
            let outcome = filesystem
                .delete_file_with_options(input, &LocalDeleteOptions::new())
                .expect("a final symlink should be removed as an entry");
            assert!(outcome.deleted());
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"keep");
            symlink_file(&target, &link).expect("file symlink should be restored");
        }
    }
}
