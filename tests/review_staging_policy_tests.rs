// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalComponentPolicy;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
#[cfg(target_os = "linux")]
use qubit_local_files::policy::LocalOwnerAccessPolicy;
use tempfile::tempdir;

fn denied_handle(root: &Path) -> qubit_local_files::directory::LocalDirectoryHandle {
    LocalFileSystem::host()
        .expect("host filesystem")
        .open_directory_with_options(
            root,
            &LocalDirectoryOpenOptions::new().with_component_policy(
                LocalComponentPolicy::deny_names(vec![OsString::from("blocked")]).expect("policy"),
            ),
        )
        .expect("policy handle")
}

fn assert_private_staging_rejects_denied_component(options: LocalTempDirectoryOptions) {
    let scratch = tempdir().expect("scratch directory");
    let parent = denied_handle(scratch.path());
    let mut stage = parent.create_staged_directory(&options).expect("stage");
    let mut input = &b"payload"[..];

    let error = stage
        .write_file_new(Path::new("blocked/../payload"), &mut input, 64)
        .expect_err("denied names remain rejected even when normalized away");

    assert_eq!(qubit_local_files::error::LocalFileErrorKind::InvalidPath, error.kind());
    assert!(input.len() == b"payload".len(), "validation must precede input reads");
    assert!(!scratch.path().join(stage.relative_name()).join("blocked").exists());
    assert!(!scratch.path().join(stage.relative_name()).join("payload").exists());
}

#[test]
fn private_staging_rejects_denied_component_before_creating_parent_or_reading_input() {
    assert_private_staging_rejects_denied_component(LocalTempDirectoryOptions::new());
}

#[cfg(target_os = "linux")]
#[test]
fn private_staging_with_exact_owner_policy_rejects_denied_component_before_side_effects() {
    assert_private_staging_rejects_denied_component(
        LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600)),
    );
}

#[test]
fn publish_rejects_denied_target_without_consuming_stage() {
    let scratch = tempdir().expect("scratch directory");
    let parent = denied_handle(scratch.path());
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let stage_name = stage.relative_name().to_os_string();

    let error = stage
        .publish_new(&parent, std::ffi::OsStr::new("blocked"), &Default::default())
        .expect_err("publish target obeys parent component policy");

    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::InvalidPath,
        error.cause().kind()
    );
    assert_eq!(
        qubit_local_files::outcome::LocalTreePublication::NotPublished,
        error.publication()
    );
    assert!(
        scratch.path().join(&stage_name).is_dir(),
        "failed publication retains the stage"
    );
    assert!(!scratch.path().join("blocked").exists());
}
