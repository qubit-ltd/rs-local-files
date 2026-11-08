// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalFileOperation;
use crate::error::LocalFileErrorKind;
use crate::options::LocalSealOptions;
use crate::options::LocalTempFileOptions;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_rlf4_sealed_observation_preserves_injected_query_context() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let filesystem = crate::LocalFileSystem::host().expect("host filesystem should open");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file should be created");
    let sealed = temporary
        .seal_with_options(&LocalSealOptions::new())
        .expect("temporary file should be sealed");
    let expected_path = sealed.path().to_path_buf();

    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 1);
    let error = sealed.observation().expect_err("injected query should fail");
    assert!(fault.fired(), "the observation injection should be consumed");
    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(expected_path.as_path()));
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    assert_eq!(
        error.io_error().expect("native source should be retained").to_string(),
        "injected native I/O contract failure"
    );
}
