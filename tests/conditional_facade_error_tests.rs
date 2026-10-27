// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
#[cfg(windows)]
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::Digest;

fn compare_facades(
    filesystem: &LocalFileSystem,
    directory: &LocalDirectoryHandle,
    relative: &Path,
    precondition: LocalWritePrecondition<sha2::digest::Output<sha2::Sha256>>,
    options: &LocalWriteOptions,
    expected: LocalFileErrorKind,
) {
    let filesystem_result = filesystem.open_conditional_writer::<sha2::Sha256>(relative, precondition.clone(), options);
    let directory_result = directory.open_conditional_writer::<sha2::Sha256>(relative, precondition, options);
    let filesystem_error = filesystem_result.expect_err("filesystem facade should reject statically");
    let directory_error = directory_result.expect_err("directory facade should reject statically");

    assert_eq!(filesystem_error.kind(), expected);
    assert_eq!(directory_error.kind(), expected);
    assert_eq!(filesystem_error.operation(), LocalFileOperation::OpenWriter);
    assert_eq!(directory_error.operation(), LocalFileOperation::OpenWriter);
    assert_eq!(filesystem_error.reason(), directory_error.reason());
}

#[test]
fn conditional_facades_share_static_errors_without_creating_resources() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject),
        )
        .expect("directory handle for same Rooted authority");
    let entries_before = std::fs::read_dir(fixture.path())
        .expect("list fixture")
        .map(|entry| entry.expect("read entry").file_name())
        .collect::<Vec<_>>();
    let missing_parent = Path::new("missing-parent/target");

    compare_facades(
        &filesystem,
        &directory,
        missing_parent,
        LocalWritePrecondition::Absent,
        &LocalWriteOptions::new(LocalWriteMode::Append),
        LocalFileErrorKind::InvalidOptions,
    );
    compare_facades(
        &filesystem,
        &directory,
        missing_parent,
        LocalWritePrecondition::Content(sha2::Sha256::digest(b"expected")),
        &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        LocalFileErrorKind::InvalidOptions,
    );

    #[cfg(windows)]
    compare_facades(
        &filesystem,
        &directory,
        missing_parent,
        LocalWritePrecondition::Content(sha2::Sha256::digest(b"expected")),
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        LocalFileErrorKind::Unsupported,
    );

    #[cfg(windows)]
    compare_facades(
        &filesystem,
        &directory,
        missing_parent,
        LocalWritePrecondition::Absent,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_durability(LocalDurabilityRequirement::Required),
        LocalFileErrorKind::RequirementNotMet,
    );

    let entries_after = std::fs::read_dir(fixture.path())
        .expect("list fixture")
        .map(|entry| entry.expect("read entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        entries_after, entries_before,
        "static errors must not stage or create parents"
    );
    assert!(!fixture.path().join("missing-parent").exists());
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
