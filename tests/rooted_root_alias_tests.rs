// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Rooted operations whose followed parent link resolves to the virtual root.

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::Path;

    use qubit_io::Output;
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::outcome::LocalPersistFailureState;
    use qubit_local_files::policy::LocalSymlinkPolicy;
    use tempfile::tempdir;

    /// Uses aliases to the virtual root as explicit and default temp parents,
    /// while preserving Reject and boundary-escape behavior.
    #[test]
    fn temp_parents_and_probes_accept_root_aliases_within_policy() {
        let directory = tempdir().expect("temporary root should be created");
        fs::create_dir(directory.path().join("child")).expect("child directory should be created");
        symlink(".", directory.path().join("alias"))
            .expect("relative root alias should be created");
        symlink("/", directory.path().join("absolute-alias"))
            .expect("absolute virtual-root alias should be created");
        symlink("child", directory.path().join("child-alias"))
            .expect("ordinary child alias should be created");
        symlink("../..", directory.path().join("escape-alias"))
            .expect("outside link fixture should be created");

        let mut rooted =
            LocalFileSystem::rooted(directory.path()).expect("root authority should open");
        let file = rooted
            .create_temp_file_with_options(
                &LocalTempFileOptions::new().with_parent(Path::new("alias")),
            )
            .expect("relative root alias should be a valid temp parent");
        assert!(file.path().starts_with(Path::new("/")));
        drop(file);
        let temp_directory = rooted
            .create_temp_directory_with_options(
                &LocalTempDirectoryOptions::new().with_parent(Path::new("absolute-alias")),
            )
            .expect("absolute virtual-root alias should be a valid temp parent");
        drop(temp_directory);

        rooted
            .set_current_directory(Path::new("alias"))
            .expect("PWD may name a link to the virtual root");
        let default_parent = rooted
            .create_temp_file()
            .expect("default parent at a root alias should use the root handle");
        drop(default_parent);

        let limits = rooted
            .limits_at(Path::new("alias"))
            .expect("root-alias limits should be observable");
        let root_limits = rooted
            .limits_at(Path::new("/"))
            .expect("root limits should be observable");
        assert_eq!(root_limits.length_unit(), limits.length_unit());
        assert_eq!(
            root_limits.max_component_length(),
            limits.max_component_length()
        );
        assert_eq!(root_limits.max_path_length(), limits.max_path_length());
        let space = rooted
            .space_at(Path::new("alias"))
            .expect("root-alias space should be observable");
        assert!(
            space.capacity_bytes().is_some()
                || space.free_bytes().is_some()
                || space.available_bytes().is_some()
        );

        let child_file = rooted
            .create_temp_file_with_options(
                &LocalTempFileOptions::new().with_parent(Path::new("child-alias")),
            )
            .expect("ordinary in-scope directory links should remain supported");
        drop(child_file);
        let escaped = rooted
            .create_temp_file_with_options(
                &LocalTempFileOptions::new().with_parent(Path::new("escape-alias")),
            )
            .expect_err("an alias escaping the opened root must remain rejected");
        assert_eq!(LocalFileErrorKind::InvalidPath, escaped.kind());

        rooted
            .set_current_directory(Path::new("/"))
            .expect("PWD should return to the virtual root");
        rooted
            .set_symlink_policy(LocalSymlinkPolicy::Reject)
            .expect("Reject policy should be selectable");
        let rejected = rooted
            .create_temp_file_with_options(
                &LocalTempFileOptions::new().with_parent(Path::new("alias")),
            )
            .expect_err("Reject policy should continue rejecting the root link");
        assert_eq!(LocalFileErrorKind::Unsupported, rejected.kind());
    }

    /// Dotted directory operands and a final link remain rejected at their own path boundary.
    #[test]
    fn directory_qualified_alias_and_final_link_operands_are_rejected() {
        let directory = tempdir().expect("temporary root should be created");
        fs::write(directory.path().join("sentinel"), b"keep").expect("sentinel should be written");
        symlink(".", directory.path().join("alias")).expect("root alias should be created");
        let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

        let remove_file = rooted
            .delete_file(Path::new("alias/."))
            .expect_err("the virtual root reached through an alias cannot be removed as a file");
        assert_eq!(LocalFileErrorKind::InvalidPath, remove_file.kind());
        let remove_directory = rooted.delete_directory(Path::new("alias/.")).expect_err(
            "the virtual root reached through an alias cannot be removed as a directory",
        );
        assert!(matches!(
            remove_directory.kind(),
            LocalFileErrorKind::InvalidPath | LocalFileErrorKind::NotDirectory
        ));
        assert!(directory.path().join("alias").symlink_metadata().is_ok());
        assert_eq!(
            b"keep",
            fs::read(directory.path().join("sentinel"))
                .unwrap()
                .as_slice()
        );

        let mut temporary = rooted
            .create_temp_file()
            .expect("temporary file should be created");
        Output::write_fully(&mut temporary, b"source")
            .expect("temporary contents should be written");
        let mut persist = temporary
            .persist(Path::new("/alias"))
            .expect_err("a root alias cannot be the final persistence target");
        assert_eq!(LocalPersistFailureState::NotPublished, persist.state());
        persist
            .resource_mut()
            .cleanup()
            .expect("rejected persistence should retain cleanup ownership");
        assert!(directory.path().is_dir());
        assert_eq!(
            b"keep",
            fs::read(directory.path().join("sentinel"))
                .unwrap()
                .as_slice()
        );
    }

    /// Literal virtual-root operands are rejected before any removal or persist publication.
    #[test]
    fn virtual_root_operands_are_rejected_for_removal_and_persistence() {
        let directory = tempdir().expect("temporary root should be created");
        fs::write(directory.path().join("sentinel"), b"keep").expect("sentinel should be written");
        let rooted = LocalFileSystem::rooted(directory.path()).expect("root authority should open");

        let remove_file = rooted
            .delete_file(Path::new("/"))
            .expect_err("the virtual root cannot be removed as a file");
        assert_eq!(LocalFileErrorKind::InvalidPath, remove_file.kind());
        let remove_directory = rooted
            .delete_directory(Path::new("/"))
            .expect_err("the virtual root cannot be removed as a directory");
        assert_eq!(LocalFileErrorKind::InvalidPath, remove_directory.kind());

        let mut temporary = rooted
            .create_temp_file()
            .expect("temporary file should be created");
        Output::write_fully(&mut temporary, b"source")
            .expect("temporary contents should be written");
        let mut persist = temporary
            .persist(Path::new("/"))
            .expect_err("the virtual root cannot be the final persistence target");
        assert_eq!(LocalPersistFailureState::NotPublished, persist.state());
        persist
            .resource_mut()
            .cleanup()
            .expect("rejected persistence should retain cleanup ownership");

        assert!(directory.path().is_dir());
        assert_eq!(
            b"keep",
            fs::read(directory.path().join("sentinel"))
                .unwrap()
                .as_slice()
        );
    }
}
