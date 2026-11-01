// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::directory::LocalStagedDirectory;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::policy::LocalPrivacyRequirement;

const CASE_ENV: &str = "QUBIT_STAGED_WRITE_FD_FAILURE_CASE";

struct ExhaustingInput {
    first: bool,
    original: libc::rlimit,
    held: Vec<fs::File>,
}

impl qubit_io::Input for ExhaustingInput {
    type Item = u8;

    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        if self.first {
            self.first = false;
            assert!(count >= 8);
            output[index..index + 8].copy_from_slice(b"partial!");
            return Ok(8);
        }
        let reduced = libc::rlimit {
            rlim_cur: self.original.rlim_cur.min(128),
            rlim_max: self.original.rlim_max,
        };
        // SAFETY: The child process alone lowers its soft descriptor limit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &reduced) }, 0);
        loop {
            match fs::File::open("/dev/null") {
                Ok(file) => self.held.push(file),
                Err(error) => {
                    assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                    break;
                }
            }
        }
        Err(io::Error::other("RLF4 input failure"))
    }
}

impl Drop for ExhaustingInput {
    fn drop(&mut self) {
        self.held.clear();
        // SAFETY: Restores this child's original descriptor limit.
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &self.original) }, 0);
    }
}

fn stage(parent: &LocalDirectoryHandle, options: &LocalTempDirectoryOptions) -> LocalStagedDirectory {
    parent
        .create_staged_directory(options)
        .expect("stage should be created")
}

#[test]
fn test_partial_private_write_retains_cleanup_owner_after_fd_exhaustion() {
    let executable = std::env::current_exe().expect("test executable");
    for policy in ["required", "exact"] {
        #[cfg(not(target_os = "linux"))]
        if policy == "exact" {
            continue;
        }
        let output = Command::new(&executable)
            .args([
                "--exact",
                "test_partial_private_write_fd_failure_child",
                "--test-threads=1",
            ])
            .env(CASE_ENV, policy)
            .output()
            .expect("isolated child should start");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("test_partial_private_write_fd_failure_child"),
            "{stdout}\n{stderr}"
        );
        assert!(
            stdout.contains("running 1 test"),
            "child must execute one test: {stdout}\n{stderr}"
        );
        assert!(output.status.success(), "{stdout}\n{stderr}");
    }
}

#[test]
fn test_partial_private_write_fd_failure_child() {
    let Ok(policy) = std::env::var(CASE_ENV) else {
        return;
    };
    let scratch = tempfile::tempdir().expect("scratch");
    fs::write(scratch.path().join("sentinel"), b"untouched").expect("sentinel");
    let parent = LocalFileSystem::host()
        .expect("host")
        .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
        .expect("directory authority");
    let options = match policy.as_str() {
        "required" => LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        #[cfg(target_os = "linux")]
        "exact" => LocalTempDirectoryOptions::new().with_owner_access_policy(
            qubit_local_files::policy::LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600),
        ),
        _ => panic!("unknown policy"),
    };
    let mut stage = stage(&parent, &options);
    let stage_path = scratch.path().join(stage.relative_name());
    let mut original = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `original` is writable resource-limit storage.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut original) }, 0);
    let mut input = ExhaustingInput {
        first: true,
        original,
        held: Vec::new(),
    };
    let error = stage
        .write_file_new(Path::new("payload"), &mut input, 64)
        .expect_err("second input read fails after writing a partial file");
    drop(input);

    assert_eq!(error.operation(), LocalFileOperation::Read);
    assert!(
        error
            .io_error()
            .expect("source error")
            .to_string()
            .contains("RLF4 input failure")
    );
    assert_eq!(
        b"untouched".as_slice(),
        fs::read(scratch.path().join("sentinel")).expect("sentinel remains")
    );
    if stage_path.join("payload").exists() {
        assert!(
            error.temp_recovery().is_some(),
            "retained residue requires recovery owner"
        );
        assert!(
            error.cleanup_error().is_some(),
            "failed cleanup requires observable cleanup error"
        );
        let (_, recovery) = error.into_temp_recovery_parts();
        let mut recovery = recovery.expect("recovery owner");
        recovery
            .cleanup()
            .expect("owner cleans partial file after FD capacity returns");
        assert!(!stage_path.join("payload").exists());
    }
    stage.cleanup().expect("stage cleanup");
}
