// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileReader;
use crate::LocalResult;
use crate::local::conditional::RecoveryAccess;
use crate::recovery::LocalRecoveryFileRole;

/// Read-only access to a file retained for recovery after a conditional write.
///
/// The namespace paths are for locating and describing the retained file.
/// Reads use the retained native parent-directory permission and verify the
/// observed entry identity; dropping this value never removes the file.
/// The owner must arrange any eventual deletion using an authorized filesystem
/// operation after inspecting the recovery role and publication state.
#[must_use]
#[derive(Debug)]
pub struct LocalRecoveryFile {
    role: LocalRecoveryFileRole,
    namespace_path: PathBuf,
    diagnostic_path: Option<PathBuf>,
    access: Box<dyn RecoveryAccess>,
}

impl LocalRecoveryFile {
    /// Creates a recovery handle from an observed entry and its retained
    /// access.
    pub(crate) fn new(
        role: LocalRecoveryFileRole,
        namespace_path: PathBuf,
        diagnostic_path: Option<PathBuf>,
        access: Box<dyn RecoveryAccess>,
    ) -> Self {
        Self {
            role,
            namespace_path,
            diagnostic_path,
            access,
        }
    }

    /// Returns the retained file's role in the conditional write attempt.
    pub const fn role(&self) -> LocalRecoveryFileRole {
        self.role
    }

    /// Returns the file's namespace path for diagnostics and user guidance.
    #[must_use]
    pub fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }

    /// Returns an optional path suitable for diagnostic display.
    #[must_use]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }

    /// Opens a reader after verifying that the retained entry still identifies
    /// the same file.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry was removed, replaced, or cannot be read.
    pub fn open_reader(&self) -> LocalResult<LocalFileReader> {
        self.access.open_reader()
    }
}

#[cfg(test)]
mod conditional_types_tests {
    use std::fs::File;
    use std::path::PathBuf;

    use qubit_io::Streams;
    use tempfile::tempdir;

    use crate::LocalFileError;
    use crate::LocalFileOperation;
    use crate::LocalFileReader;
    use crate::LocalResult;
    use crate::local::conditional::ConditionalEntry;
    use crate::local::conditional::RecoveryAccess;
    use crate::recovery::LocalRecoveryFileRole;

    #[derive(Debug)]
    struct FakeRecoveryAccess {
        path: PathBuf,
    }

    impl RecoveryAccess for FakeRecoveryAccess {
        fn open_reader(&self) -> LocalResult<LocalFileReader> {
            let file = File::open(&self.path).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::OpenReader, Some(self.path.clone()), None, error)
            })?;
            LocalFileReader::from_file(file).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::OpenReader, Some(self.path.clone()), None, error)
            })
        }
    }

    #[test]
    fn recovery_file_drop_keeps_file_and_reader_works() {
        let directory = tempdir().expect("create temporary directory");
        let path = directory.path().join("recovery");
        std::fs::write(&path, b"candidate B").expect("write recovery content");
        let entry = ConditionalEntry::new(
            LocalRecoveryFileRole::CandidateCopy,
            path.clone(),
            Some(path.clone()),
            Box::new(FakeRecoveryAccess { path: path.clone() }),
        );

        let recovery = entry.into_recovery(LocalRecoveryFileRole::CandidateCopy);
        assert_eq!(recovery.role(), LocalRecoveryFileRole::CandidateCopy);
        assert_eq!(recovery.namespace_path(), path);
        assert_eq!(recovery.diagnostic_path(), Some(path.as_path()));
        let mut reader = recovery.open_reader().expect("open retained recovery file");
        let mut contents = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut contents).expect("read recovery content");
        assert_eq!(contents, b"candidate B");
        drop(recovery);

        assert_eq!(std::fs::read(path).expect("recovery file remains"), b"candidate B");
    }
}
