// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::LocalFileSystem;
use crate::directory::DirectoryReaderPathBinding;
use crate::directory::LocalComponentPolicy;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;
use crate::options::LocalObservedReadOptions;

#[test]
fn test_fixed_reader_binding_reopens_its_recipe_and_redacts_paths() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("payload"), b"data").expect("payload should be written");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let options = LocalObservedReadOptions::new().with_native_observation(LocalObservationRequirement::Required);
    let reader = filesystem
        .open_observed_reader(Path::new("/payload"), &options)
        .expect("observed reader should open");
    let identity = reader
        .observation()
        .identity()
        .cloned()
        .expect("required observation should include identity");
    let request_path = fixture.path().join("payload");
    let binding = DirectoryReaderPathBinding::new(
        filesystem.clone(),
        Path::new("payload").to_path_buf(),
        request_path.clone(),
        LocalComponentPolicy::allow_all(),
        options,
        identity,
        None,
    );

    assert_eq!(None, binding.verification_deadline());
    assert_eq!(LocalSourceVerification::Unchanged, binding.verify_until(None).unwrap());
    filesystem
        .open_observed_reader_fixed_with_deadline(
            Path::new("payload"),
            &request_path,
            &LocalComponentPolicy::allow_all(),
            &options,
            None,
            None,
        )
        .expect("fixed reader should open with an explicit verification deadline");
    let debug = format!("{binding:?}");
    assert!(debug.contains("DirectoryReaderPathBinding"));
    assert!(!debug.contains(fixture.path().to_string_lossy().as_ref()));

    let expired = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let error = binding.verify_until(Some(expired)).unwrap_err();
    assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
    assert_eq!(Some(request_path.as_path()), error.path());
}
