// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::time::Duration;
use std::time::Instant;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalSealOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalTempSourceState;

const CHILD: &str = "QUBIT_LF_REV_Q01_CHILD";

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[derive(Clone, Copy)]
enum Replacement {
    Normal,
    RegularFile,
    Fifo,
}

fn run_case(test_name: &str, rooted: bool, replacement: Replacement) {
    if let Some(parent) = std::env::var_os(CHILD) {
        let parent = PathBuf::from(parent);
        let filesystem = if rooted {
            LocalFileSystem::rooted(&parent).expect("Rooted filesystem should open")
        } else {
            LocalFileSystem::host().expect("Host filesystem should open")
        };
        let temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(if rooted {
                Path::new("/")
            } else {
                &parent
            }))
            .expect("temporary should exist");
        let path = temporary.path().to_path_buf();
        let physical_path = if rooted {
            parent.join(
                path.strip_prefix(Path::new("/"))
                    .expect("Rooted temporary path is virtual-absolute"),
            )
        } else {
            path.clone()
        };

        match replacement {
            Replacement::Normal => {
                let mut sealed = temporary
                    .seal_with_options(&LocalSealOptions::new())
                    .expect("unchanged file should seal");
                sealed.cleanup().expect("sealed resource should clean up");
                assert!(!physical_path.exists());
                write_marker(&parent, b"sealed-cleaned");
            }
            Replacement::RegularFile | Replacement::Fifo => {
                std::fs::remove_file(&physical_path).expect("remove own fixture entry");
                if matches!(replacement, Replacement::RegularFile) {
                    std::fs::write(&physical_path, b"replacement contents")
                        .expect("replacement file should be written");
                } else {
                    let native = CString::new(physical_path.as_os_str().as_bytes()).expect("fixture has no NUL");
                    // SAFETY: native is a live NUL-terminated fixture path.
                    assert_eq!(unsafe { libc::mkfifo(native.as_ptr(), 0o600) }, 0);
                }

                let error = temporary
                    .seal_with_options(&LocalSealOptions::new())
                    .expect_err("replaced entry must be rejected");
                let resource = error.into_resource();
                assert_eq!(resource.path(), path);
                assert_eq!(resource.source_state(), LocalTempSourceState::Owned);
                drop(resource);
                match replacement {
                    Replacement::RegularFile => assert_eq!(
                        std::fs::read(&physical_path).expect("replacement remains"),
                        b"replacement contents",
                    ),
                    Replacement::Fifo => assert!(
                        std::fs::symlink_metadata(&physical_path)
                            .expect("replacement remains")
                            .file_type()
                            .is_fifo()
                    ),
                    Replacement::Normal => unreachable!(),
                }
                write_marker(&parent, b"rejected");
            }
        }
        return;
    }

    let fixture = tempfile::tempdir().expect("parent owns fixture");
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().expect("test binary"))
            .args(["--exact", test_name, "--nocapture"])
            .env(CHILD, fixture.path())
            .spawn()
            .expect("child should start"),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait().expect("child status") {
            assert!(status.success(), "child failed: {status}");
            let marker = std::fs::read(fixture.path().join("child-completed"))
                .expect("child executed and wrote completion marker");
            let expected = if matches!(replacement, Replacement::Normal) {
                b"sealed-cleaned".as_slice()
            } else {
                b"rejected".as_slice()
            };
            assert_eq!(marker, expected);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Q01: {test_name} child failed to return before watchdog"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn write_marker(parent: &Path, value: &[u8]) {
    std::fs::write(parent.join("child-completed"), value).expect("completion marker");
}

#[test]
fn test_host_seal_unchanged_file_succeeds() {
    run_case("test_host_seal_unchanged_file_succeeds", false, Replacement::Normal);
}

#[test]
fn test_host_seal_rejects_regular_file_replacement() {
    run_case(
        "test_host_seal_rejects_regular_file_replacement",
        false,
        Replacement::RegularFile,
    );
}

#[test]
fn test_host_seal_rejects_fifo_replacement_without_blocking() {
    run_case(
        "test_host_seal_rejects_fifo_replacement_without_blocking",
        false,
        Replacement::Fifo,
    );
}

#[test]
fn test_rooted_seal_unchanged_file_succeeds() {
    run_case("test_rooted_seal_unchanged_file_succeeds", true, Replacement::Normal);
}

#[test]
fn test_rooted_seal_rejects_regular_file_replacement() {
    run_case(
        "test_rooted_seal_rejects_regular_file_replacement",
        true,
        Replacement::RegularFile,
    );
}

#[test]
fn test_rooted_seal_rejects_fifo_replacement_without_blocking() {
    run_case(
        "test_rooted_seal_rejects_fifo_replacement_without_blocking",
        true,
        Replacement::Fifo,
    );
}
