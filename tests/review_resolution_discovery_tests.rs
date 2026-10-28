// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalResult;
use qubit_local_files::directory::LocalComponentPolicy;
use qubit_local_files::directory::LocalResolutionOrigin;
use qubit_local_files::directory::LocalResolutionPolicy;
use qubit_local_files::directory::LocalResolutionScope;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalRootOpenOptions;
#[cfg(unix)]
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

#[derive(Debug, Default)]
struct PrivateRecorder(Mutex<Vec<(String, LocalResolutionScope, LocalResolutionOrigin)>>);

impl LocalResolutionPolicy for PrivateRecorder {
    fn validate_component(
        &self,
        component: &OsStr,
        scope: LocalResolutionScope,
        origin: LocalResolutionOrigin,
    ) -> LocalResult<()> {
        self.0.lock().expect("hook record should be available").push((
            component.to_string_lossy().into_owned(),
            scope,
            origin,
        ));
        if component == OsStr::new("private") {
            return Err(LocalFileError::new(
                LocalFileErrorKind::PermissionDenied,
                LocalFileOperation::BindPath,
            ));
        }
        Ok(())
    }

    fn validate_symlink_target(&self, _: &Path, _: LocalResolutionScope) -> LocalResult<()> {
        Ok(())
    }
}

fn rooted_handle(
    root: &Path,
    component_policy: LocalComponentPolicy,
) -> qubit_local_files::directory::LocalDirectoryHandle {
    LocalFileSystem::rooted(root)
        .expect("root authority should open")
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_component_policy(component_policy),
        )
        .expect("root directory handle should open")
}

#[test]
fn component_policy_does_not_filter_discovered_tree_copy_entries() {
    let fixture = tempdir().expect("fixture root should be created");
    fs::create_dir_all(fixture.path().join("source/private")).expect("private subtree should be created");
    fs::write(fixture.path().join("source/private/payload"), b"contents").expect("payload should be written");
    let handle = rooted_handle(
        fixture.path(),
        LocalComponentPolicy::deny_names(vec!["private".into()]).expect("component policy should be valid"),
    );

    let error = handle
        .open_observed_reader(Path::new("source/private/payload"), &LocalObservedReadOptions::new())
        .expect_err("direct access should reject the denied component");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());

    let entries = handle
        .list_with_options(Path::new("source"), &LocalListOptions::new().with_recursive())
        .expect("listing root should bind")
        .map(|entry| entry.expect("recursive discovery should complete"))
        .map(|entry| entry.relative_path().to_path_buf())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|path| path == Path::new("private/payload")));

    let outcome = handle
        .copy_with_options(
            Path::new("source"),
            Path::new("copy"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("tree copy should process discovered entries without filtering them");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        b"contents",
        fs::read(fixture.path().join("copy/private/payload"))
            .expect("discovered private payload should be copied")
            .as_slice()
    );
}

#[test]
fn resolution_policy_applies_to_resolver_paths_but_not_native_recursive_enumeration() {
    let fixture = tempdir().expect("fixture root should be created");
    fs::create_dir_all(fixture.path().join("copy-source/private")).expect("copy subtree should be created");
    fs::write(fixture.path().join("copy-source/private/payload"), b"copied").expect("copy payload should be written");
    fs::create_dir_all(fixture.path().join("delete-tree/private")).expect("delete subtree should be created");
    fs::write(fixture.path().join("delete-tree/private/payload"), b"deleted")
        .expect("delete payload should be written");

    let policy = Arc::new(PrivateRecorder::default());
    let filesystem = LocalFileSystem::rooted_with_options(
        fixture.path(),
        &LocalRootOpenOptions::new().with_resolution_policy(policy.clone()),
    )
    .expect("root authority with resolver policy should open");

    let direct_error = filesystem
        .open_observed_reader(
            Path::new("copy-source/private/payload"),
            &LocalObservedReadOptions::new(),
        )
        .expect_err("direct resolver access should reject private");
    assert_eq!(LocalFileErrorKind::PermissionDenied, direct_error.kind());
    assert!(
        policy
            .0
            .lock()
            .expect("hook record should be available")
            .iter()
            .any(|(component, _, origin)| component == "private" && *origin == LocalResolutionOrigin::Input)
    );

    let handle = filesystem
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_resolution_policy(policy.clone()),
        )
        .expect("policy-bearing directory handle should open");
    let mut walker = handle
        .list_with_options(Path::new("copy-source"), &LocalListOptions::new())
        .expect("walker should bind its explicit starting path");
    let walk_error = walker
        .find_map(Result::err)
        .expect("walker should resolve each discovered child path");
    assert_eq!(LocalFileErrorKind::PermissionDenied, walk_error.kind());

    let recorded_before_native_recursion = policy
        .0
        .lock()
        .expect("hook record should be available")
        .iter()
        .filter(|(component, _, _)| component == "private")
        .count();
    let outcome = handle
        .copy_with_options(
            Path::new("copy-source"),
            Path::new("copy-target"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("tree copy should enumerate descendants through rooted native readers");
    assert_eq!(1, outcome.stats().files());
    assert_eq!(
        b"copied",
        fs::read(fixture.path().join("copy-target/private/payload"))
            .expect("native tree copy should read discovered payload")
            .as_slice()
    );
    assert!(
        filesystem
            .delete_directory_with_options(Path::new("delete-tree"), &LocalDeleteOptions::new().with_recursive())
            .expect("recursive delete should enumerate descendants through rooted native readers")
            .deleted()
    );
    assert!(!fixture.path().join("delete-tree").exists());

    let recorded_after_native_recursion = policy
        .0
        .lock()
        .expect("hook record should be available")
        .iter()
        .filter(|(component, _, _)| component == "private")
        .count();
    assert_eq!(
        recorded_before_native_recursion, recorded_after_native_recursion,
        "copy and recursive delete should not synthesize resolver visits for native recursive entries"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        fs::create_dir_all(fixture.path().join("private")).expect("symlink target directory should be created");
        fs::write(fixture.path().join("private/linked-payload"), b"linked")
            .expect("symlink target payload should be written");
        symlink(
            "../private/linked-payload",
            fixture.path().join("copy-source/private-link"),
        )
        .expect("discovered link should be created");

        let link_copy_error = handle
            .copy_with_options(
                Path::new("copy-source"),
                Path::new("copy-following-link"),
                &LocalCopyOptions::new()
                    .with_tree_source()
                    .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
            )
            .expect_err("resolving a discovered link target should run the inherited policy");
        assert_eq!(LocalFileErrorKind::PermissionDenied, link_copy_error.error().kind());
        assert!(
            policy
                .0
                .lock()
                .expect("hook record should be available")
                .iter()
                .any(|(component, _, origin)| {
                    component == "private" && *origin == LocalResolutionOrigin::SymlinkTarget
                })
        );
    }
}
