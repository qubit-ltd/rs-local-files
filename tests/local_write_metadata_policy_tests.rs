// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic metadata policy is independent of publication and identity checks.

use std::fs;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

/// Windows Host merge preserves an old alternate stream; UseStaging does not
/// request a merge.
#[cfg(windows)]
#[test]
fn test_windows_host_metadata_policy_selects_native_merge_behavior() {
    for policy in [
        LocalWriteMetadataPolicy::PreserveExisting,
        LocalWriteMetadataPolicy::UseStaging,
    ] {
        let fixture = tempfile::tempdir().expect("isolated Windows fixture");
        let target = fixture.path().join("destination");
        fs::write(&target, b"old").expect("existing destination");
        let mut stream_name = target.as_os_str().to_os_string();
        stream_name.push(":metadata-marker");
        let stream = Path::new(&stream_name);
        fs::write(stream, b"old stream metadata").expect("NTFS alternate data stream");
        let filesystem = LocalFileSystem::host().expect("Host filesystem");
        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy);
        let mut writer = filesystem
            .open_writer_with_options(&target, &options)
            .expect("staged writer");
        writer.write_all(b"new").expect("staged content");
        assert!(writer.commit().expect("native replacement").atomic());
        assert_eq!(fs::read(&target).expect("published content"), b"new");
        if policy == LocalWriteMetadataPolicy::PreserveExisting {
            assert_eq!(fs::read(stream).expect("merged stream"), b"old stream metadata");
        } else {
            assert_eq!(
                fs::read(stream).expect_err("old stream must not be merged").kind(),
                std::io::ErrorKind::NotFound
            );
        }
    }
}

/// Staging metadata replacement remains private until atomic commit in both
/// scopes.
#[test]
fn test_use_staging_replaces_content_after_commit() {
    for rooted in [false, true] {
        let dir = tempfile::tempdir().expect("isolated fixture");
        let physical = dir.path().join("target");
        fs::write(&physical, b"old").expect("old content");
        let filesystem = if rooted {
            LocalFileSystem::rooted(dir.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let path = if rooted {
            Path::new("target")
        } else {
            physical.as_path()
        };
        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
            .with_metadata_policy(LocalWriteMetadataPolicy::UseStaging);
        let mut writer = filesystem
            .open_writer_with_options(path, &options)
            .expect("staging writer");
        writer.write_all(b"new").expect("staged content");
        assert_eq!(fs::read(&physical).expect("old content before commit"), b"old");
        assert!(writer.commit().expect("atomic commit").atomic());
        assert_eq!(fs::read(&physical).expect("published content"), b"new");
    }
}

/// Staging policy does not inherit old file permissions on Unix.
#[cfg(unix)]
#[test]
fn test_metadata_policy_selects_permissions() {
    use std::os::unix::fs::PermissionsExt;
    for rooted in [false, true] {
        for policy in [
            LocalWriteMetadataPolicy::PreserveExisting,
            LocalWriteMetadataPolicy::UseStaging,
        ] {
            let dir = tempfile::tempdir().expect("isolated fixture");
            let physical = dir.path().join("target");
            fs::write(&physical, b"old").expect("old content");
            fs::set_permissions(&physical, fs::Permissions::from_mode(0o700)).expect("old permissions");
            let filesystem = if rooted {
                LocalFileSystem::rooted(dir.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let path = if rooted {
                Path::new("target")
            } else {
                physical.as_path()
            };
            let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy);
            let mut writer = filesystem
                .open_writer_with_options(path, &options)
                .expect("staging writer");
            writer.write_all(b"new").expect("staged content");
            assert!(writer.commit().expect("commit").atomic());
            let actual = fs::metadata(&physical).expect("permissions").permissions().mode() & 0o777;
            if policy == LocalWriteMetadataPolicy::PreserveExisting {
                assert_eq!(actual, 0o700);
            } else {
                assert_eq!(actual & 0o111, 0, "staging must not acquire old executable bits");
            }
        }
    }
}

/// Skipping metadata copying still executes the existing identity fault
/// boundaries.
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_use_staging_keeps_identity_checks_and_skips_metadata_reads() {
    use qubit_local_files::outcome::LocalWriteFailureState;
    use qubit_local_files::test_support::install_test_fault;
    for rooted in [false, true] {
        let identity_fault = if rooted {
            "rooted-identity-mismatch"
        } else {
            "atomic-identity-mismatch"
        };
        let open_fault = if rooted {
            "rooted-destination-open"
        } else {
            "atomic-destination-open"
        };
        for fault in [
            identity_fault,
            open_fault,
            "atomic-metadata-owner",
            "atomic-metadata-source-stat",
        ] {
            for policy in [
                LocalWriteMetadataPolicy::PreserveExisting,
                LocalWriteMetadataPolicy::UseStaging,
            ] {
                let dir = tempfile::tempdir().expect("isolated fixture");
                let physical = dir.path().join("target");
                fs::write(&physical, b"old").expect("old content");
                let filesystem = if rooted {
                    LocalFileSystem::rooted(dir.path())
                } else {
                    LocalFileSystem::host()
                }
                .expect("filesystem");
                let path = if rooted {
                    Path::new("target")
                } else {
                    physical.as_path()
                };
                let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy);
                let mut writer = filesystem
                    .open_writer_with_options(path, &options)
                    .expect("staging writer");
                writer.write_all(b"new").expect("staged content");
                let guard = install_test_fault(fault).expect("fault controller");
                let result = writer.commit();
                drop(guard);
                if fault == identity_fault || policy == LocalWriteMetadataPolicy::PreserveExisting {
                    let error = result.expect_err("identity or preservation fault must fail");
                    assert_eq!(error.state(), LocalWriteFailureState::NotPublished);
                    let (_error, _state, retained) = error.into_parts();
                    let mut writer = retained.expect("prepublication writer remains recoverable");
                    let _ = writer.abort().expect("abort retained staging");
                    assert_eq!(fs::read(&physical).expect("untouched target"), b"old");
                } else {
                    assert!(result.expect("metadata fault must not affect UseStaging").atomic());
                    assert_eq!(fs::read(&physical).expect("published target"), b"new");
                }
            }
        }
    }
}

/// Create-new never replaces a concurrent creator under either metadata policy.
#[test]
fn test_metadata_policy_keeps_create_new_conflicts() {
    for policy in [
        LocalWriteMetadataPolicy::PreserveExisting,
        LocalWriteMetadataPolicy::UseStaging,
    ] {
        for rooted in [false, true] {
            let dir = tempfile::tempdir().expect("isolated fixture");
            let physical = dir.path().join("target");
            let filesystem = if rooted {
                LocalFileSystem::rooted(dir.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let path = if rooted {
                Path::new("target")
            } else {
                physical.as_path()
            };
            let options = LocalWriteOptions::new(LocalWriteMode::CreateNew).with_metadata_policy(policy);
            let mut writer = filesystem
                .open_writer_with_options(path, &options)
                .expect("new staging writer");
            writer.write_all(b"ours").expect("staged content");
            fs::write(&physical, b"concurrent").expect("concurrent creator");
            assert!(writer.commit().is_err());
            assert_eq!(fs::read(&physical).expect("concurrent content"), b"concurrent");
        }
    }
}

/// Append keeps direct-write and abort semantics regardless of metadata policy.
#[test]
fn test_metadata_policy_does_not_change_append() {
    for policy in [
        LocalWriteMetadataPolicy::PreserveExisting,
        LocalWriteMetadataPolicy::UseStaging,
    ] {
        let dir = tempfile::tempdir().expect("isolated fixture");
        let physical = dir.path().join("target");
        fs::write(&physical, b"old").expect("old content");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let options = LocalWriteOptions::new(LocalWriteMode::Append).with_metadata_policy(policy);
        let mut writer = filesystem
            .open_writer_with_options(&physical, &options)
            .expect("append writer");
        writer.write_all(b"+").expect("append bytes");
        let _ = writer.abort().expect("abort releases append handle");
        assert_eq!(fs::read(&physical).expect("appended content"), b"old+");
    }
}

/// A real non-root child verifies replacement of an unreadable writable file.
#[cfg(unix)]
#[test]
fn test_use_staging_does_not_require_old_read_access() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "RS_LOCAL_FILES_METADATA_PERMISSION_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args([
                "--exact",
                "test_use_staging_does_not_require_old_read_access",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()
            .expect("permission child");
        assert!(status.success(), "non-root permission child must pass");
        return;
    }
    // SAFETY: geteuid takes no arguments and has no memory or ownership contract.
    assert_ne!(
        unsafe { libc::geteuid() },
        0,
        "permission contract requires a non-root test process"
    );
    for rooted in [false, true] {
        for policy in [
            LocalWriteMetadataPolicy::PreserveExisting,
            LocalWriteMetadataPolicy::UseStaging,
        ] {
            let dir = tempfile::tempdir().expect("isolated permission fixture");
            let physical = dir.path().join("target");
            fs::write(&physical, b"old").expect("old content");
            fs::set_permissions(&physical, fs::Permissions::from_mode(0o200)).expect("write-only old target");
            let filesystem = if rooted {
                LocalFileSystem::rooted(dir.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let path = if rooted {
                Path::new("target")
            } else {
                physical.as_path()
            };
            let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy);
            let mut writer = filesystem
                .open_writer_with_options(path, &options)
                .expect("staging writer");
            writer.write_all(b"new").expect("staged content");
            let result = writer.commit();
            fs::set_permissions(&physical, fs::Permissions::from_mode(0o600)).expect("restore inspection permission");
            if policy == LocalWriteMetadataPolicy::PreserveExisting {
                assert!(result.is_err(), "old metadata requires readable old handle");
                assert_eq!(fs::read(&physical).expect("old target"), b"old");
            } else {
                assert!(result.expect("replacement without old read access").atomic());
                assert_eq!(fs::read(&physical).expect("new target"), b"new");
            }
        }
    }
}

/// Both metadata policies retain Published after a required parent-sync
/// failure.
#[cfg(all(unix, feature = "test-support"))]
#[test]
fn test_metadata_policy_preserves_published_sync_failure() {
    use qubit_local_files::outcome::LocalWriteFailureState;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault;
    for rooted in [false, true] {
        for policy in [
            LocalWriteMetadataPolicy::PreserveExisting,
            LocalWriteMetadataPolicy::UseStaging,
        ] {
            let dir = tempfile::tempdir().expect("isolated fixture");
            let physical = dir.path().join("new/parent/target");
            let filesystem = if rooted {
                LocalFileSystem::rooted(dir.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let path = if rooted {
                Path::new("new/parent/target")
            } else {
                physical.as_path()
            };
            let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
                .with_create_parent()
                .with_durability(LocalDurabilityRequirement::Required)
                .with_metadata_policy(policy);
            let mut writer = filesystem
                .open_writer_with_options(path, &options)
                .expect("staging writer");
            writer.write_all(b"published").expect("staged content");
            let fault = if rooted {
                "rooted-preferred-parent-sync"
            } else {
                "atomic-writer-created-parent-sync"
            };
            let guard = install_test_fault(fault).expect("parent-sync fault");
            let error = writer.commit().expect_err("required parent sync must fail");
            drop(guard);
            assert_eq!(error.state(), LocalWriteFailureState::Published);
            assert_eq!(fs::read(&physical).expect("published content"), b"published");
        }
    }
}
