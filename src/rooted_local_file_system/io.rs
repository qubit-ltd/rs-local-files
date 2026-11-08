// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted io operations.

use std::time::Instant;

use super::Arc;
use super::LocalDirectoryWalker;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileWriter;
use super::LocalListOptions;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalWriteMode;
use super::LocalWriteOptions;
use super::Path;
use super::RootedLocalFileSystem;
use super::ensure_required_directory_durability;
use super::io;
use super::resolve_rooted_path;
use super::resolve_rooted_path_with_deadline;
use super::rooted_io_error;
use super::validate_rooted_list_start;

impl RootedLocalFileSystem {
    /// Opens a descriptor-relative reader for a rooted regular file.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    /// - `options`: Reader open policy.
    /// - `symlink_policy`: Intermediate and final link-resolution policy.
    ///
    /// # Returns
    ///
    /// Owned regular-file reader.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for lexical escape, rejected symlink traversal,
    /// invalid entry kinds, or native open failures.
    pub fn open_reader(
        &self,
        path: &Path,
        options: &LocalReadOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        self.open_reader_with_contract(path, options, symlink_policy, self.root.resolution_contract().clone())
    }

    /// Opens a reader using an existing resolution contract.
    ///
    /// The contract supplies the path-resolution limits and policies shared
    /// with the caller; this helper does not replace or broaden them.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    /// - `options`: Reader open policy.
    /// - `symlink_policy`: Intermediate and final link-resolution policy.
    /// - `contract`: Existing resolution limits and policies.
    ///
    /// # Returns
    ///
    /// An owned regular-file reader when the rooted path can be opened.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for path, policy, type, or native open
    /// failures.
    pub(crate) fn open_reader_with_contract(
        &self,
        path: &Path,
        options: &LocalReadOptions,
        symlink_policy: LocalSymlinkPolicy,
        contract: crate::resolution::ResolutionContract,
    ) -> LocalResult<LocalFileReader> {
        self.open_reader_with_contract_and_deadline(path, options, symlink_policy, contract, None)
    }

    /// Opens a reader after resolving its rooted path within an existing
    /// absolute operation deadline.
    ///
    /// The deadline is shared with the caller and is never restarted. `None`
    /// adds no deadline. Resolution checks an expired deadline and returns a
    /// `TimedOut` error with the existing operation and path context.
    ///
    /// # Parameters
    /// - `path`: Validated relative descendant path.
    /// - `options`: Reader open policy.
    /// - `symlink_policy`: Intermediate and final link-resolution policy.
    /// - `contract`: Existing resolution limits and policies.
    /// - `deadline`: Optional absolute deadline for this operation.
    ///
    /// # Returns
    /// An owned regular-file reader when resolution and opening complete in
    /// time.
    ///
    /// # Errors
    /// Returns `LocalFileError` for path, policy, type, native open, or timeout
    /// failures. Timeout errors retain the `OpenReader` operation and input
    /// path.
    pub(crate) fn open_reader_with_contract_and_deadline(
        &self,
        path: &Path,
        options: &LocalReadOptions,
        symlink_policy: LocalSymlinkPolicy,
        contract: crate::resolution::ResolutionContract,
        deadline: Option<Instant>,
    ) -> LocalResult<LocalFileReader> {
        let root = crate::rooted::Root::from_directory_authority_with_contract(
            self.root.try_clone_authority().map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenReader, Some(path.to_path_buf()), None, source)
            })?,
            self.root.path().to_path_buf(),
            contract,
        );
        let relative = resolve_rooted_path_with_deadline(
            &root,
            path,
            symlink_policy,
            true,
            LocalFileOperation::OpenReader,
            deadline,
        )?;
        super::support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenReader)?;
        let native_options = options
            .open_retry_timeout()
            .map_or_else(crate::read::OpenOptions::default, |timeout| {
                crate::read::OpenOptions::default().with_open_retry_timeout(timeout)
            });
        let metadata = root
            .symlink_metadata(&relative)
            .map_err(|error| rooted_io_error(LocalFileOperation::OpenReader, path, error))?;
        if metadata.kind() != crate::rooted::EntryKind::File {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenReader)
                    .with_path(path.to_path_buf()),
            );
        }
        #[cfg(test)]
        crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
            crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::ObservedReaderAfterMetadataBeforeContentOpen,
        );
        super::support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenReader)?;
        root.open_reader(&relative, &native_options)
            .and_then(LocalFileReader::from_file)
            .map_err(|error| rooted_io_error(LocalFileOperation::OpenReader, path, error))
    }

    /// Creates a descriptor-relative lazy directory walker.
    ///
    /// # Parameters
    ///
    /// - `path`: Relative directory path, or an empty path for the authority
    ///   root.
    /// - `namespace_root`: Public namespace-absolute listing root.
    /// - `options`: Traversal policy and optional per-operation link override.
    /// - `symlink_policy`: Default link policy when options contain no
    ///   override.
    ///
    /// # Returns
    ///
    /// A walker whose descendant operations derive from the opened authority.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, scope escapes, or
    /// native directory-read failures.
    pub fn list(
        &self,
        path: &Path,
        namespace_root: &Path,
        options: &LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
        started_at: Instant,
    ) -> LocalResult<LocalDirectoryWalker> {
        let symlink_policy = options.symlink_policy().unwrap_or(symlink_policy);
        let deadline = match options.deadline() {
            Some(duration) => Some(started_at.checked_add(duration).ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::List)
                    .with_reason("listing deadline is not representable")
                    .with_path(path.to_path_buf())
            })?),
            None => None,
        };
        let resolved = validate_rooted_list_start(&self.root, path, symlink_policy, deadline)?;
        let authority_path = if resolved.as_os_str().is_empty() {
            None
        } else {
            Some(crate::local::LocalRelativePath::new(&resolved).map_err(|error| {
                rooted_io_error(LocalFileOperation::List, path, error).with_kind(LocalFileErrorKind::InvalidPath)
            })?)
        };
        LocalDirectoryWalker::open_rooted(
            Arc::clone(&self.root),
            authority_path,
            namespace_root.to_path_buf(),
            *options,
            symlink_policy,
            started_at,
        )
    }

    /// Opens a descriptor-relative writer publication session.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative destination path.
    /// - `options`: Publication mode and guarantee policy.
    /// - `symlink_policy`: Path-resolution policy; CreateNew preserves the
    ///   final entry, while other modes apply the policy to that entry too.
    ///
    /// # Returns
    ///
    /// A stateful rooted writer.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, conflicts, unsupported
    /// required atomicity for append, invalid entry kinds, or native open
    /// failures.
    pub fn open_writer(
        &self,
        path: &Path,
        options: &LocalWriteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileWriter> {
        use crate::writer::internal::LocalFileWriterBackend;

        if options.mode() == LocalWriteMode::Append && options.atomicity() == crate::LocalAtomicityRequirement::Required
        {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter)
                    .with_reason("append mode cannot provide required atomic publication")
                    .with_path(path.to_path_buf()),
            );
        }
        if options.mode() != LocalWriteMode::Append {
            ensure_required_directory_durability(
                options.durability(),
                LocalFileOperation::OpenWriter,
                path,
                path,
                self.capabilities.supports_durable_write(),
                "required directory durability is unavailable for this rooted authority",
            )?;
        }
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            options.mode() != LocalWriteMode::CreateNew,
            LocalFileOperation::OpenWriter,
        )?;
        let backend = match options.mode() {
            LocalWriteMode::CreateNew | LocalWriteMode::CreateOrReplace => {
                let mut atomic_options = crate::LocalAtomicWriteOptions::new()
                    .with_durability(options.durability())
                    .with_file_sync(options.file_sync())
                    .with_metadata_policy(options.metadata_policy());
                if options.mode() == LocalWriteMode::CreateNew {
                    atomic_options = atomic_options.with_create_new();
                }
                if options.creates_parent() {
                    atomic_options = atomic_options.with_create_parent();
                }
                if let Some(timeout) = options.open_retry_timeout() {
                    atomic_options = atomic_options.with_open_retry_timeout(timeout);
                }
                let writer = self
                    .root
                    .begin_atomic_write_with_options(&relative, atomic_options)
                    .map_err(|error| {
                        let kind = error.kind();
                        rooted_io_error(LocalFileOperation::OpenWriter, path, io::Error::new(kind, error))
                    })?;
                LocalFileWriterBackend::Rooted(writer)
            }
            LocalWriteMode::Append => {
                let metadata = self
                    .root
                    .symlink_metadata(&relative)
                    .map_err(|error| rooted_io_error(LocalFileOperation::OpenWriter, path, error))?;
                if metadata.kind() != crate::rooted::EntryKind::File {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenWriter)
                            .with_path(path.to_path_buf()),
                    );
                }
                let mut native_options = crate::write::OpenOptions::new(crate::write::Mode::AppendExisting);
                if let Some(timeout) = options.open_retry_timeout() {
                    native_options = native_options.with_open_retry_timeout(timeout);
                }
                let file = self
                    .root
                    .open_writer(&relative, &native_options)
                    .map_err(|error| rooted_io_error(LocalFileOperation::OpenWriter, path, error))?;
                LocalFileWriterBackend::Append(file)
            }
        };
        let diagnostic = self.root.path().join(relative.as_path());
        Ok(LocalFileWriter::new(diagnostic, backend, *options))
    }
}

#[cfg(test)]
mod deadline_tests {
    use std::path::Path;
    use std::time::Duration;
    use std::time::Instant;

    use super::RootedLocalFileSystem;
    use crate::LocalFileOperation;
    use crate::LocalListOptions;
    use crate::LocalReadOptions;
    use crate::LocalSymlinkPolicy;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;
    use crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard;

    #[test]
    fn test_rlf5_observed_rooted_helper_checks_deadline_during_resolution() {
        let scratch = tempfile::tempdir().expect("fixture directory should be created");
        std::fs::create_dir(scratch.path().join("nested")).expect("nested directory should be created");
        std::fs::write(scratch.path().join("nested/item"), b"payload").expect("fixture file should exist");
        let rooted = RootedLocalFileSystem::open(scratch.path()).expect("rooted authority should open");
        let deadline = Instant::now() + Duration::from_secs(60);
        let _clock = DeadlineClockGuard::expire_after(1);

        let error = rooted
            .open_reader_with_contract_and_deadline(
                Path::new("nested/item"),
                &LocalReadOptions::new(),
                LocalSymlinkPolicy::FollowWithinScope,
                rooted.root.resolution_contract().clone(),
                Some(deadline),
            )
            .expect_err("deadline must be checked between resolution steps");

        assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
        assert_eq!(LocalFileOperation::OpenReader, error.operation());
    }

    #[test]
    fn test_rlf5_observed_rooted_helper_stops_after_metadata_before_content_open() {
        let scratch = tempfile::tempdir().expect("fixture directory should be created");
        std::fs::write(scratch.path().join("item"), b"payload").expect("fixture file should exist");
        let rooted = RootedLocalFileSystem::open(scratch.path()).expect("rooted authority should open");
        let deadline = Instant::now() + Duration::from_secs(60);
        let _clock = DeadlineClockGuard::expire_at(
            crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::ObservedReaderAfterMetadataBeforeContentOpen,
        );

        let error = rooted
            .open_reader_with_contract_and_deadline(
                Path::new("item"),
                &LocalReadOptions::new(),
                LocalSymlinkPolicy::FollowWithinScope,
                rooted.root.resolution_contract().clone(),
                Some(deadline),
            )
            .expect_err("expired deadline must stop before opening file contents");

        assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
        assert_eq!(LocalFileOperation::OpenReader, error.operation());
        assert_eq!(Some(Path::new("item")), error.path());
    }

    #[test]
    fn test_reader_authority_clone_failure_keeps_the_input_path() {
        let scratch = tempfile::tempdir().expect("fixture directory should be created");
        std::fs::write(scratch.path().join("item"), b"payload").expect("fixture file should exist");
        let rooted = RootedLocalFileSystem::open(scratch.path()).expect("rooted authority should open");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);

        let error = rooted
            .open_reader_with_contract_and_deadline(
                Path::new("item"),
                &LocalReadOptions::new(),
                LocalSymlinkPolicy::FollowWithinScope,
                rooted.root.resolution_contract().clone(),
                None,
            )
            .expect_err("authority clone failure must reject opening the reader");

        assert!(guard.fired());
        assert_eq!(LocalFileOperation::OpenReader, error.operation());
        assert_eq!(Some(Path::new("item")), error.path());
        assert_eq!(std::io::ErrorKind::Other, error.io_error_kind());
    }

    #[test]
    fn test_listing_rejects_an_unrepresentable_relative_deadline() {
        let scratch = tempfile::tempdir().expect("fixture directory should be created");
        let rooted = RootedLocalFileSystem::open(scratch.path()).expect("rooted authority should open");

        let error = rooted
            .list(
                Path::new(""),
                Path::new("/"),
                &LocalListOptions::new().with_deadline(Duration::MAX),
                LocalSymlinkPolicy::FollowWithinScope,
                Instant::now(),
            )
            .expect_err("unrepresentable deadline must be rejected before listing");

        assert_eq!(LocalFileOperation::List, error.operation());
        assert_eq!(crate::LocalFileErrorKind::InvalidOptions, error.kind());
        assert_eq!(Some(Path::new("")), error.path());
    }
}
