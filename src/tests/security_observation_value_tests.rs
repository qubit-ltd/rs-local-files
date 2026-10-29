// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Contracts for composing and reading native security observation values.

#[test]
fn test_shared_filesystem_core_debug_retains_the_operational_snapshot() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    for filesystem in [
        crate::LocalFileSystem::host().expect("host should open"),
        crate::LocalFileSystem::rooted(fixture.path()).expect("root should open"),
    ] {
        let rendered = format!("{:?}", filesystem.core);
        assert!(rendered.starts_with("LocalFileSystemCore { namespace:"));
        assert!(rendered.contains("capabilities:"));
        assert!(rendered.contains("limits:"));
        assert!(!rendered.contains("conditional_hooks"));
        assert!(!rendered.contains("test_faults"));
        assert_eq!(format!("{:?}", filesystem.clone().core), rendered);
    }
}

#[cfg(unix)]
#[test]
fn test_unix_observation_values_preserve_platform_facts_and_absent_windows_facts() {
    use crate::observation::LocalAccessObservation;
    use crate::observation::LocalAclPresence;
    use crate::observation::LocalFileSystemLocality;
    use crate::observation::LocalFileSystemObservation;
    use crate::observation::LocalNativeFileDetails;
    use crate::observation::LocalUnixAccessObservation;
    use crate::observation::LocalUnixFileDetails;
    use crate::observation::LocalUnixFileSystemObservation;
    use crate::observation::LocalUnixPlatform;

    for (platform, type_id, name) in [
        (LocalUnixPlatform::Linux, 0xEF53, None),
        (LocalUnixPlatform::Darwin, 0x8000_0000, Some("apfs".to_owned())),
    ] {
        let unix = LocalUnixFileSystemObservation::new(platform, type_id, name.clone());
        let facts = LocalFileSystemObservation::new(LocalFileSystemLocality::Local, Some(unix), None);
        assert_eq!(facts.locality(), LocalFileSystemLocality::Local);
        assert!(facts.windows().is_none());
        let unix = facts.unix().expect("Unix filesystem facts");
        assert_eq!(unix.platform(), platform);
        assert_eq!(unix.type_id(), type_id);
        assert_eq!(unix.type_name(), name.as_deref());
    }
    let access = LocalAccessObservation::new(
        Some(LocalUnixAccessObservation::new(
            0o4750,
            false,
            LocalAclPresence::Present,
            LocalAclPresence::NotSupported,
        )),
        None,
    );
    assert!(access.windows().is_none());
    let unix = access.unix().expect("Unix access facts");
    assert_eq!(unix.mode(), 0o4750);
    assert!(!unix.owner_is_current_user());
    assert_eq!(unix.access_acl(), LocalAclPresence::Present);
    assert_eq!(unix.default_acl(), LocalAclPresence::NotSupported);

    let details = LocalNativeFileDetails::from_unix(LocalUnixFileDetails::from_parts(123, 456, -2, 3, -4, 5));
    assert!(details.windows().is_none());
    let unix = details.unix().expect("Unix native details");
    assert_eq!(unix.device(), 123);
    assert_eq!(unix.inode(), 456);
    assert_eq!(unix.modified_seconds(), -2);
    assert_eq!(unix.modified_nanos(), 3);
    assert_eq!(unix.changed_seconds(), -4);
    assert_eq!(unix.changed_nanos(), 5);
}

#[test]
fn test_security_observation_link_count_requirement_defaults_and_override() {
    use crate::observation::LocalObservationRequirement;
    use crate::observation::LocalSecurityObservationOptions;

    assert_eq!(
        LocalSecurityObservationOptions::new().link_count(),
        LocalObservationRequirement::Optional
    );
    let options = LocalSecurityObservationOptions::new().with_link_count(LocalObservationRequirement::Required);
    assert_eq!(options.link_count(), LocalObservationRequirement::Required);
}

#[test]
fn test_snapshot_into_bytes_preserves_owned_binary_content() {
    use crate::LocalFileKind;
    use crate::LocalFileMetadata;
    use crate::LocalFilePermissions;
    use crate::observation::LocalFileObservation;
    use crate::observation::LocalFileSnapshot;

    let bytes = vec![0, 0xff, 0x80, b'\n'];
    let metadata = LocalFileMetadata::from_parts(
        LocalFileKind::File,
        bytes.len() as u64,
        None,
        None,
        None,
        LocalFilePermissions::new(false, None),
    );
    let before = LocalFileObservation::from_parts(metadata, None, None);
    let after = before.clone();
    let snapshot = LocalFileSnapshot::new(bytes.clone(), before, after);
    assert_eq!(snapshot.bytes(), bytes);
    assert_eq!(snapshot.before().metadata().len(), 4);
    assert_eq!(snapshot.after().metadata().len(), 4);
    assert_eq!(snapshot.into_bytes(), bytes);
}
