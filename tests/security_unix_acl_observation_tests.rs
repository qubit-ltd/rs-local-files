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
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::observation::LocalAclPresence;
use qubit_local_files::observation::LocalObservationRequirement;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalObservedReadOptions;

fn install_extended_acl(path: &Path) -> Vec<u8> {
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
    acl
}

#[test]
fn test_required_security_observation_reports_mode_owner_and_acl_from_reader() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("private-data");
    std::fs::write(&path, b"private payload").expect("fixture should be written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).expect("fixture mode should be set");

    // Install a valid extended ACL through the real filesystem interface. This
    // verifies the public observation path reports ACL presence from the
    // retained reader descriptor rather than only validating synthetic bytes.
    let acl = install_extended_acl(&path);

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

#[test]
fn test_required_security_observation_rejects_acl_byte_limit_without_partial_facts() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("bounded-acl");
    std::fs::write(&path, b"private payload").expect("fixture should be written");
    let acl = install_extended_acl(&path);
    let reader = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("reader should open");

    let error = reader
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required)
                .with_max_acl_bytes(acl.len() - 1),
        )
        .expect_err("an ACL larger than the configured byte budget must fail");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit,
        error.kind()
    );
    let limit = error.resource_limit_error().expect("ACL byte limit remains structured");
    assert_eq!(qubit_local_files::error::LocalResourceKind::AclBytes, limit.resource());
    assert_eq!(Some(acl.len()), limit.observed());
}

#[test]
fn test_required_security_observation_rejects_acl_entry_limit_without_partial_facts() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("bounded-acl-entries");
    std::fs::write(&path, b"private payload").expect("fixture should be written");
    install_extended_acl(&path);
    let reader = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("reader should open");

    let error = reader
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_access(LocalObservationRequirement::Required)
                .with_max_acl_entries(4),
        )
        .expect_err("five ACL entries must exceed a four-entry budget");
    assert_eq!(
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit,
        error.kind()
    );
    let limit = error
        .resource_limit_error()
        .expect("ACL entry limit remains structured");
    assert_eq!(
        qubit_local_files::error::LocalResourceKind::AclEntries,
        limit.resource()
    );
    assert_eq!(Some(5), limit.observed());
}
