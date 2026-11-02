// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use crate::LocalFileError;
#[cfg(feature = "test-support")]
use crate::LocalFileOperation;
use crate::temp::types::NativePrivateEntry;

pub(super) fn entry(parent: File, name: &std::ffi::OsStr, file: File, directory: bool) -> NativePrivateEntry {
    let identity =
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Optional)
            .ok()
            .and_then(|observation| observation.identity().cloned());
    NativePrivateEntry {
        parent: Arc::new(parent),
        name: name.to_os_string(),
        file: Some(Arc::new(file)),
        identity,
        directory,
    }
}

pub(super) fn finish(primary: LocalFileError, entries: Vec<(PathBuf, NativePrivateEntry)>) -> LocalFileError {
    if entries.is_empty() {
        return primary;
    }
    let diagnostic_path = entries[0].0.clone();
    let mut recovery = crate::LocalTempRecovery::from_native_entries_with_paths(diagnostic_path.clone(), entries);
    #[cfg(feature = "test-support")]
    let cleanup = if crate::local::test_support_enabled("rooted-temp-sandbox-cleanup")
        || crate::local::test_support_enabled("temp-identity-file-cleanup")
        || crate::local::test_support_enabled("rooted-temp-file-open-cleanup")
        || crate::local::test_support_enabled("rooted-temp-directory-create-cleanup")
        || crate::local::test_support_enabled("rooted-temp-file-collision-cleanup")
        || crate::local::test_support_enabled("rooted-temp-directory-collision-cleanup")
        || crate::local::test_support_enabled("rooted-temp-file-owner-cleanup")
        || crate::local::test_support_enabled("rooted-temp-directory-owner-cleanup")
        || crate::local::test_support_enabled("rooted-temp-file-recovery-clone-cleanup")
        || crate::local::test_support_enabled("rooted-temp-directory-recovery-clone-cleanup")
    {
        Err(LocalFileError::from_io(
            LocalFileOperation::Cleanup,
            Some(diagnostic_path.clone()),
            None,
            std::io::Error::other("injected sandbox cleanup failure"),
        ))
    } else {
        recovery.cleanup()
    };
    #[cfg(not(feature = "test-support"))]
    let cleanup = recovery.cleanup();
    match cleanup {
        Ok(()) => primary,
        Err(cleanup_error) => primary.with_cleanup_error(cleanup_error).with_temp_recovery(recovery),
    }
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
