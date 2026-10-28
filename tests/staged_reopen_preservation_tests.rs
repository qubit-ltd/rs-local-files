// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Failed staged-directory reopen must leave the recovery tree intact.

use std::fs;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreeValidationOptions;
use qubit_local_files::outcome::LocalFileKind;
use tempfile::TempDir;

fn native_stage(parent: &TempDir, name: &std::ffi::OsStr) -> std::path::PathBuf {
    parent.path().join(name)
}

fn failed_reopen_preserves_tree(rooted: bool, options: LocalTreeValidationOptions, mismatched_identity: bool) {
    let scratch = tempfile::tempdir().expect("scratch parent");
    let fs = if rooted {
        LocalFileSystem::rooted(scratch.path())
    } else {
        LocalFileSystem::host()
    }
    .expect("filesystem");
    let parent = fs
        .open_directory_with_options(
            if rooted {
                Path::new(MAIN_SEPARATOR_STR)
            } else {
                scratch.path()
            },
            &LocalDirectoryOpenOptions::new(),
        )
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"recovery bytes"[..], 64)
        .expect("payload");
    let name = stage.relative_name().to_os_string();
    let native = native_stage(&scratch, &name);
    let identity = if mismatched_identity {
        let other = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("other stage");
        let result = other.identity().expect("identity").clone();
        other.cleanup().expect("other cleanup");
        result
    } else {
        stage.identity().expect("identity").clone()
    };
    stage.cleanup().expect("close and remove original stage");

    // Recreate a byte-identical directory at the recorded name so the test's
    // recovery bytes are independent of the stage object's cleanup owner.
    fs::create_dir(&native).expect("recovery directory");
    fs::write(native.join("payload"), b"recovery bytes").expect("recovery payload");
    let current = parent
        .observe_entry(Path::new(&name))
        .expect("current stage identity")
        .identity()
        .expect("current identity")
        .clone();
    let identity = if mismatched_identity { identity } else { current };
    let result = parent.open_staged_directory(Path::new(&name), &identity, &options);
    if mismatched_identity {
        assert_eq!(
            LocalFileErrorKind::RequirementNotMet,
            result.expect_err("identity mismatch should fail").kind()
        );
    } else {
        match result {
            Err(_) => {}
            Ok(stage) => {
                drop(stage.retain_in_place());
                panic!("validation should fail: options={options:?}");
            }
        }
    }
    assert_eq!(
        b"recovery bytes",
        fs::read(native.join("payload")).expect("bytes remain").as_slice()
    );
    assert!(native.join("payload").exists(), "recovery entry remains");
}

#[test]
fn test_failed_reopen_preserves_host_and_rooted_recovery_trees() {
    for rooted in [false, true] {
        let empty = LocalTreeValidationOptions::new().with_exact_entries(Vec::new());
        failed_reopen_preserves_tree(rooted, empty, false);

        let duplicate = LocalTreeEntry::new("payload".into(), LocalFileKind::File).expect("entry");
        let duplicate_options =
            LocalTreeValidationOptions::new().with_exact_entries(vec![duplicate.clone(), duplicate]);
        failed_reopen_preserves_tree(rooted, duplicate_options, false);

        let zero_budget = LocalTreeValidationOptions::new()
            .with_exact_entries(vec![
                LocalTreeEntry::new("payload".into(), LocalFileKind::File).expect("entry"),
            ])
            .with_limits(LocalListOptions::new().with_max_entries(0));
        failed_reopen_preserves_tree(rooted, zero_budget, false);

        failed_reopen_preserves_tree(
            rooted,
            LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0)),
            false,
        );
        failed_reopen_preserves_tree(
            rooted,
            LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(0)),
            false,
        );
        failed_reopen_preserves_tree(
            rooted,
            LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_deadline(Duration::ZERO)),
            false,
        );

        failed_reopen_preserves_tree(rooted, LocalTreeValidationOptions::new(), true);
    }
}

#[cfg(unix)]
#[test]
fn test_failed_reopen_rejects_symlink_special_and_external_hardlink_without_cleanup() {
    use std::os::unix::fs::symlink;

    for rooted in [false, true] {
        for (entry_name, options, setup) in [
            ("link", LocalTreeValidationOptions::new().with_reject_symlinks(), 0_u8),
            (
                "fifo",
                LocalTreeValidationOptions::new().with_reject_special_files(),
                1_u8,
            ),
            (
                "payload",
                LocalTreeValidationOptions::new().with_reject_external_hardlinks(),
                2_u8,
            ),
        ] {
            let scratch = tempfile::tempdir().expect("scratch parent");
            let fs = if rooted {
                LocalFileSystem::rooted(scratch.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem");
            let parent = fs
                .open_directory_with_options(
                    if rooted {
                        Path::new(MAIN_SEPARATOR_STR)
                    } else {
                        scratch.path()
                    },
                    &LocalDirectoryOpenOptions::new(),
                )
                .expect("parent authority");
            let mut stage = parent
                .create_staged_directory(&LocalTempDirectoryOptions::new())
                .expect("stage");
            stage
                .write_file_new(Path::new("payload"), &mut &b"recovery bytes"[..], 64)
                .expect("payload");
            let name = stage.relative_name().to_os_string();
            let native = native_stage(&scratch, &name);
            let identity = stage.identity().expect("identity").clone();
            drop(stage.retain_in_place());

            match setup {
                0 => symlink("payload", native.join(entry_name)).expect("symlink fixture"),
                1 => {
                    let fifo = std::ffi::CString::new(native.join(entry_name).as_os_str().as_encoded_bytes())
                        .expect("fifo path");
                    assert_eq!(
                        0,
                        unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) },
                        "fifo fixture should be created"
                    );
                }
                2 => std::fs::hard_link(native.join("payload"), scratch.path().join("outside"))
                    .expect("outside hardlink"),
                _ => unreachable!(),
            }

            let result = parent.open_staged_directory(Path::new(&name), &identity, &options);
            match result {
                Err(error) => {
                    assert!(matches!(
                        error.kind(),
                        LocalFileErrorKind::RequirementNotMet | LocalFileErrorKind::ResourceLimit
                    ));
                }
                Ok(stage) => {
                    drop(stage.retain_in_place());
                    panic!("enabled rejection option must reject fixture {entry_name}");
                }
            }
            assert_eq!(
                b"recovery bytes".as_slice(),
                std::fs::read(native.join("payload"))
                    .expect("recovery bytes remain")
                    .as_slice()
            );
        }
    }
}

#[test]
fn test_successful_reopen_cleanup_and_drop_still_clean_up() {
    for rooted in [false, true] {
        let scratch = tempfile::tempdir().expect("scratch parent");
        let fs = if rooted {
            LocalFileSystem::rooted(scratch.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let parent = fs
            .open_directory_with_options(
                if rooted {
                    Path::new(MAIN_SEPARATOR_STR)
                } else {
                    scratch.path()
                },
                &LocalDirectoryOpenOptions::new(),
            )
            .expect("parent authority");
        for explicit_cleanup in [true, false] {
            let mut stage = parent
                .create_staged_directory(&LocalTempDirectoryOptions::new())
                .expect("stage");
            stage
                .write_file_new(Path::new("payload"), &mut &b"ok"[..], 8)
                .expect("payload");
            let name = stage.relative_name().to_os_string();
            let native = native_stage(&scratch, &name);
            stage.cleanup().expect("initial stage cleanup");
            fs::create_dir(&native).expect("recreate stage");
            fs::write(native.join("payload"), b"ok").expect("recreate payload");
            let identity = parent
                .observe_entry(Path::new(&name))
                .expect("identity observation")
                .identity()
                .expect("identity")
                .clone();
            let reopened = parent
                .open_staged_directory(Path::new(&name), &identity, &LocalTreeValidationOptions::new())
                .expect("reopen");
            if explicit_cleanup {
                reopened.cleanup().expect("reopened cleanup");
            } else {
                drop(reopened);
            }
            assert!(!native.exists(), "successful reopened owner cleans up");
        }
    }
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
