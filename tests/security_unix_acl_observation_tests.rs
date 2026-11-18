// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(target_os = "linux")]

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::observation::LocalAclPresence;
use qubit_local_files::observation::LocalObservationRequirement;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalObservedReadOptions;

#[test]
fn test_required_security_observation_reports_mode_owner_and_acl_from_reader() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-data");
    std::fs::write(&path, b"private payload").expect("fixture should be written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("fixture mode should be set");

    // Install a valid extended ACL through the real filesystem interface. This
    // verifies the public observation path reports ACL presence from the
    // retained reader descriptor rather than only validating synthetic bytes.
    let native_path = CString::new(path.as_os_str().as_bytes()).expect("temporary path has no NUL");
    let acl_name = c"system.posix_acl_access";
    let acl = [
        2_u32.to_le_bytes().as_slice(),
        &[0x01, 0, 6, 0, 0xff, 0xff, 0xff, 0xff],
        &[0x02, 0, 4, 0, 0x40, 0xe2, 0x01, 0],
        &[0x04, 0, 4, 0, 0xff, 0xff, 0xff, 0xff],
        &[0x10, 0, 4, 0, 0xff, 0xff, 0xff, 0xff],
        &[0x20, 0, 0, 0, 0xff, 0xff, 0xff, 0xff],
    ]
    .concat();
    // SAFETY: both pointers remain valid for the call, and `acl` is a valid
    // POSIX ACL v2 value containing the required base entries, named user, and
    // mask.
    let status = unsafe {
        libc::setxattr(
            native_path.as_ptr(),
            acl_name.as_ptr(),
            acl.as_ptr().cast(),
            acl.len(),
            0,
        )
    };
    assert_eq!(status, 0, "runner filesystem must support POSIX ACL fixtures");

    let metadata = std::fs::metadata(&path).expect("fixture metadata should be available");
    let reader = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("reader should open");
    let options = LocalSecurityObservationOptions::new()
        .with_access(LocalObservationRequirement::Required)
        .with_max_acl_entries(5)
        .with_max_acl_bytes(acl.len());

    let security = reader
        .security_observation(&options)
        .expect("required Unix access facts should be available");
    let access = security
        .access()
        .and_then(|access| access.unix())
        .expect("Unix access facts should be reported");
    assert_eq!(access.mode(), metadata.mode() & 0o7777);
    assert_eq!(access.mode(), 0o640);
    assert_eq!(
        access.owner_is_current_user(),
        metadata.uid() == unsafe { libc::geteuid() }
    );
    assert_eq!(access.access_acl(), LocalAclPresence::Present);
    assert_eq!(access.default_acl(), LocalAclPresence::Absent);
}
