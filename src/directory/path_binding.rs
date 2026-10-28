// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::LocalFileSystem;
use crate::LocalResult;
use crate::observation::LocalSourceVerification;
use crate::observation::reader::ReaderPathBinding;

#[derive(Debug)]
pub(crate) struct PathBinding {
    filesystem: LocalFileSystem,
    name: OsString,
    identity: crate::observation::LocalFileIdentity,
    options: crate::options::LocalPathBindingOptions,
}

impl PathBinding {
    pub(crate) fn new(
        filesystem: LocalFileSystem,
        name: OsString,
        identity: crate::observation::LocalFileIdentity,
        options: crate::options::LocalPathBindingOptions,
    ) -> Self {
        Self {
            filesystem,
            name,
            identity,
            options,
        }
    }
}

impl ReaderPathBinding for PathBinding {
    fn verification_deadline(&self) -> Option<Duration> {
        self.options.deadline()
    }

    fn verify_until(&self, deadline: Option<Instant>) -> LocalResult<LocalSourceVerification> {
        let result = self.filesystem.open_observed_reader_with_binding_deadline(
            Path::new(&self.name),
            &crate::options::LocalObservedReadOptions::new(),
            self.options,
            deadline,
        );
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Metadata,
                None,
                None,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
            ));
        }
        match result {
            Ok(reader) => Ok(if reader.observation().identity() == Some(&self.identity) {
                LocalSourceVerification::Unchanged
            } else {
                LocalSourceVerification::Changed
            }),
            Err(error) if error.kind() == crate::error::LocalFileErrorKind::NotFound => {
                Ok(LocalSourceVerification::Missing)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PathBinding;
    use crate::LocalFileSystem;
    use crate::observation::reader::ReaderPathBinding;
    use crate::options::LocalObservedReadOptions;
    use crate::options::LocalPathBindingOptions;

    #[test]
    fn path_binding_reopen_obeys_verification_deadline() {
        let directory = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(directory.path().join("source"), b"source").expect("source should exist");
        let filesystem = LocalFileSystem::rooted(directory.path()).expect("root should open");
        let reader = filesystem
            .open_observed_reader(std::path::Path::new("source"), &LocalObservedReadOptions::new())
            .expect("reader should open");
        let binding = PathBinding::new(
            filesystem,
            "source".into(),
            reader
                .observation()
                .identity()
                .cloned()
                .expect("identity should be available"),
            LocalPathBindingOptions::new().with_deadline(std::time::Duration::ZERO),
        );
        let error = binding.verify().unwrap_err();
        assert_eq!(error.io_error().unwrap().kind(), std::io::ErrorKind::TimedOut);
    }
}
