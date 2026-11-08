// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

use tempfile::tempfile;

use crate::LocalFileReader;
use crate::error::LocalFileErrorKind;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalObservedReader;

#[test]
fn test_observed_reader_refreshes_same_handle_and_keeps_initial_value() {
    let mut file = tempfile().expect("create temporary file");
    file.write_all(b"old").expect("write initial bytes");
    let reader = LocalFileReader::from_file(file.try_clone().expect("clone handle")).expect("construct reader");
    let observed =
        LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required).expect("observe initial file");
    let initial = observed.observation().clone();

    file.set_len(0).expect("truncate file");
    file.seek(SeekFrom::Start(0)).expect("rewind file");
    file.write_all(b"new content").expect("write replacement contents");

    let refreshed = observed.observe_now().expect("refresh retained handle");
    assert_eq!(initial.metadata().len(), 3);
    assert_eq!(observed.observation().metadata().len(), 3);
    assert_eq!(refreshed.metadata().len(), 11);
    assert_eq!(
        observed.verify_path_binding().unwrap_err().kind(),
        LocalFileErrorKind::InvalidState
    );
}
