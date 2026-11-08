// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_io::Input;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::policy::LocalSymlinkPolicy;

/// Creates a host filesystem that rejects symbolic links for conditional tests.
#[must_use]
pub fn host() -> LocalFileSystem {
    let mut filesystem = LocalFileSystem::host().expect("host filesystem");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject symlinks");
    filesystem
}

/// Reads the verified recovery candidate attached to a conditional commit
/// error.
#[must_use]
pub fn read_candidate(error: &qubit_local_files::error::LocalConditionalCommitError) -> Vec<u8> {
    let candidate = error.candidate().expect("verified candidate copy");
    read_recovery(candidate)
}

/// Reads all bytes from a retained recovery file for conditional test
/// assertions.
#[must_use]
pub fn read_recovery(file: &qubit_local_files::recovery::LocalRecoveryFile) -> Vec<u8> {
    let mut reader = file.open_reader().expect("open recovery file through retained access");
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 128];
    loop {
        match reader.read(&mut buffer).expect("read candidate") {
            0 => return bytes,
            count => bytes.extend_from_slice(&buffer[..count]),
        }
    }
}
