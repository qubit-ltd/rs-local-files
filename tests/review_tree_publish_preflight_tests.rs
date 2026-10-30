// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(windows)]

use std::ffi::OsStr;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::outcome::LocalTreeAccessState;
use qubit_local_files::outcome::LocalTreeMutationState;
use qubit_local_files::outcome::LocalTreePublication;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::policy::LocalTreeFinalAccess;

#[test]
fn test_tree_publish_required_durability_rejects_before_access_mutation() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("payload"), &mut &b"unchanged payload"[..], 64)
        .expect("payload should be staged");
    let identity = stage.identity().expect("stage identity").clone();
    let mut payload_reader = stage
        .directory()
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("payload should open");
    let payload_security_before = payload_reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("payload security should be observable");
    let payload_before = payload_reader
        .read_to_end_limited(u64::MAX)
        .expect("payload should read");
    assert!(
        payload_security_before.identity().is_some(),
        "payload identity should be available"
    );

    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new()
                .with_access(LocalTreeFinalAccess::CurrentUserOnly)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect_err("Windows cannot provide required durable rename");

    assert_eq!(error.cause().kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.mutation_state(), LocalTreeMutationState::NotStarted);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(error.access_state(), LocalTreeAccessState::Preserved);
    assert!(!fixture.path().join("published").exists());

    let mut stage = error.into_stage().expect("failed publish returns stage owner");
    assert_eq!(stage.identity(), Some(&identity));
    let payload_security_after = payload_reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("same payload handle security should remain observable");
    assert_eq!(
        payload_security_after, payload_security_before,
        "payload access facts must not change"
    );
    assert_eq!(payload_security_after.identity(), payload_security_before.identity());
    let mut payload_after_reader = stage
        .directory()
        .open_observed_reader(Path::new("payload"), &LocalObservedReadOptions::new())
        .expect("payload should remain readable");
    let payload_after = payload_after_reader
        .read_to_end_limited(u64::MAX)
        .expect("payload should remain readable");
    assert_eq!(payload_after, payload_before);
    stage.cleanup().expect("returned stage can be explicitly cleaned up");
}
