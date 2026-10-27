// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.

use super::HostLocalFileSystem;
use super::Input;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileKind;
use super::LocalFileMetadata;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileSystem;
use super::LocalFileWriter;
use super::LocalNamespace;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalWriteOptions;
use super::Path;
use super::operation_error;
use super::reject_directory_qualified_file;
use super::resolve_operation_path;
use super::with_current_directory;
#[cfg(feature = "test-support")]
use crate::local::take_test_support;
use crate::local_file_system_validation::validate_write_options;
use crate::directory::binding::PathBinding;
use crate::directory::binding::DirectoryPathBinding;
use crate::options::LocalObservedReadOptions;
use crate::rooted::Root;
use crate::path::LocalNamespacePath;
use crate::test_support::TestFaultPoint;
use crate::options::{LocalReadAllOptions, LocalSnapshotOptions};
use crate::observation::{LocalObservedReader, LocalObservationRequirement};

impl LocalFileSystem {
    /// Opens a reader with same-handle observations and optional name binding.
    pub fn open_observed_reader(&self, path: &Path, options: &LocalObservedReadOptions) -> LocalResult<LocalObservedReader> {
        let resolver_started = std::time::Instant::now();
        let read_options = options.read_options().unwrap_or(&self.defaults.read);
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
        let reader = self.open_resolved_reader(&resolved, read_options, LocalFileOperation::OpenReader, resolver.current_directory())?;
        let mut observed = LocalObservedReader::from_reader(reader, options.native_observation())?;
        if let Some(binding_options) = options.path_binding() {
            let binding_deadline = binding_options.deadline().and_then(|limit| resolver_started.checked_add(limit));
            let path_bytes = resolved.namespace_absolute().as_os_str().len();
            let raw_component_bytes = path.components().filter_map(|component| match component {
                std::path::Component::Normal(name) => Some(name.len().saturating_add(1)),
                _ => None,
            }).fold(0_usize, usize::saturating_add);
            let ancestors = resolved.namespace_absolute().components().count().saturating_sub(1);
            if binding_options.max_path_bytes().is_some_and(|limit| path_bytes > limit)
                || binding_options.max_path_bytes().is_some_and(|limit| raw_component_bytes > limit)
                || binding_options.max_ancestors().is_some_and(|limit| ancestors > limit)
                || binding_options.max_ancestors() == Some(0)
                || binding_options.deadline().is_some_and(|limit| resolver_started.elapsed() >= limit)
            {
                return Err(LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenReader)
                    .with_path(resolved.namespace_absolute().to_path_buf()));
            }
            let identity = observed.observation().identity().cloned().ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
            })?;
            let name = resolved.authority_relative().file_name().ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenReader)
                    .with_path(resolved.namespace_absolute().to_path_buf())
            })?.to_os_string();
            let parent_path = resolved.authority_relative().parent().unwrap_or_else(|| Path::new(""));
            let (parent_root, parent_anchors) = match &self.core.namespace {
                LocalNamespace::Host => {
                    let host_root = Root::open(Path::new("/")).map_err(|source| LocalFileError::from_io(
                        LocalFileOperation::OpenRoot, Some(parent_path.to_path_buf()), None, source,
                    ))?;
                    let relative_parent = parent_path.strip_prefix("/").unwrap_or(parent_path);
                    let policy = if self.symlink_policy == crate::LocalSymlinkPolicy::FollowAcrossScope {
                        crate::LocalSymlinkPolicy::FollowWithinScope
                    } else { self.symlink_policy };
                    let allow_all = crate::directory::LocalComponentPolicy::allow_all();
                    let relative_parent = crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &host_root, relative_parent, policy, true, LocalFileOperation::OpenRoot, &allow_all,
                        binding_deadline, binding_options.max_path_bytes(),
                    )?;
                    let diagnostic_parent = Path::new("/").join(&relative_parent);
                    let (root, _, anchors) = crate::directory::retain_directory_chain(
                        &host_root, &relative_parent, &diagnostic_parent, true, Some(binding_options), resolver_started,
                    )?;
                    (root, anchors.unwrap_or_default())
                }
                LocalNamespace::Rooted(rooted) => {
                    let allow_all = crate::directory::LocalComponentPolicy::allow_all();
                    let resolved_parent = crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        rooted.root_authority(), parent_path, self.symlink_policy, true, LocalFileOperation::OpenRoot,
                        &allow_all, binding_deadline, binding_options.max_path_bytes(),
                    )?;
                    let diagnostic_parent = rooted.diagnostic_path().join(&resolved_parent);
                    let (root, _, anchors) = crate::directory::retain_directory_chain(
                        rooted.root_authority(), &resolved_parent, &diagnostic_parent, true, Some(binding_options), resolver_started,
                    )?;
                    (root, anchors.unwrap_or_default())
                }
            };
            let binding_policy = if self.symlink_policy == crate::LocalSymlinkPolicy::FollowAcrossScope {
                crate::LocalSymlinkPolicy::FollowWithinScope
            } else { self.symlink_policy };
            let parent_fs = LocalFileSystem::from_root_authority(parent_root, binding_policy)?;
            if binding_options.deadline().is_some_and(|limit| resolver_started.elapsed() >= limit) {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::OpenReader,
                    Some(resolved.namespace_absolute().to_path_buf()),
                    None,
                    std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
                ));
            }
            observed = observed.with_binding(Box::new(PathBinding::new(parent_fs, name, identity, binding_options.deadline())));
            observed = observed.with_binding(Box::new(DirectoryPathBinding::new(parent_anchors, binding_options.deadline())));
        }
        Ok(observed)
    }

    /// Reads metadata for the final target while honoring this filesystem's link policy.
    ///
    /// Returns file or directory metadata. Link rejection, missing targets,
    /// resolution errors, and native metadata failures are returned as
    /// `LocalFileError` with metadata-operation context.
    pub fn target_metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                let entry = HostLocalFileSystem::metadata_with_policy(
                    resolved.authority_relative(),
                    self.symlink_policy,
                )?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Metadata,
                    )
                    .with_reason("target metadata requires following a symbolic link")
                    .with_path(resolved.namespace_absolute().to_path_buf()));
                }
                if entry.kind() == LocalFileKind::Symlink && self.symlink_policy.follows() {
                    std::fs::metadata(resolved.authority_relative())
                        .map(|metadata| LocalFileMetadata::from_native(&metadata))
                        .map_err(|error| {
                            LocalFileError::from_io(
                                LocalFileOperation::Metadata,
                                Some(resolved.namespace_absolute().to_path_buf()),
                                None,
                                error,
                            )
                        })
                } else {
                    Ok(entry)
                }
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.target_metadata(resolved.authority_relative(), self.symlink_policy)
            }
        }
    }

    /// Resolves an existing operand to its namespace coordinate.
    ///
    /// Host results are native canonical paths; Rooted results are virtual
    /// absolute paths and do not grant authority for a later operation.
    /// Returns path-policy, missing-target, or native resolution errors.
    pub fn canonicalize(&self, path: &Path) -> LocalResult<std::path::PathBuf> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                let entry = HostLocalFileSystem::metadata_with_policy(
                    resolved.authority_relative(),
                    self.symlink_policy,
                )?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Metadata,
                    )
                    .with_reason("canonicalization requires following a symbolic link")
                    .with_path(resolved.namespace_absolute().to_path_buf()));
                }
                std::fs::canonicalize(resolved.authority_relative()).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(resolved.namespace_absolute().to_path_buf()),
                        None,
                        source,
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.canonicalize_virtual(resolved.authority_relative(), self.symlink_policy)
            }
        }
    }

    /// Reads a regular file completely using the filesystem's reader defaults.
    pub fn read_all(&self, path: &Path) -> LocalResult<Vec<u8>> {
        self.read_all_with_options(path, &LocalReadAllOptions::new())
    }

    /// Reads a regular file completely with an optional byte limit.
    ///
    /// Returns `ResourceLimit` when the observed length or bytes read exceed
    /// the configured maximum. Other open/read failures retain structured
    /// operation context; partial bytes are discarded.
    pub fn read_all_with_options(
        &self,
        path: &Path,
        options: &LocalReadAllOptions,
    ) -> LocalResult<Vec<u8>> {
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let mut reader = self.open_resolved_reader_with_policy(
            &resolved,
            options.read_options().unwrap_or(&self.defaults.read),
            LocalFileOperation::Read,
            resolver.current_directory(),
            options.symlink_policy().unwrap_or(self.symlink_policy),
        )?;
        let maximum = options.max_bytes();
        if maximum.is_some_and(|limit| reader.metadata().len() > limit) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ResourceLimit,
                LocalFileOperation::Read,
            )
            .with_path(resolved.namespace_absolute().to_path_buf()));
        }
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        crate::observation::reader::collect_limited(&mut reader, maximum)
            .map_err(|error| error.with_path(resolved.namespace_absolute().to_path_buf()))
    }

    /// Reads a complete UTF-8 text file using the filesystem's reader defaults.
    pub fn read_text(&self, path: &Path) -> LocalResult<String> {
        self.read_text_with_options(path, &LocalReadAllOptions::new())
    }

    /// Reads a complete UTF-8 text file with an optional byte limit.
    ///
    /// Returns `InvalidEncoding` with the original UTF-8 conversion error
    /// when the complete file is not valid UTF-8.
    pub fn read_text_with_options(
        &self,
        path: &Path,
        options: &LocalReadAllOptions,
    ) -> LocalResult<String> {
        let bytes = self.read_all_with_options(path, options)?;
        String::from_utf8(bytes).map_err(|error| {
            LocalFileError::from_utf8(LocalFileOperation::Read, Some(path.to_path_buf()), error)
        })
    }

    /// Reads bytes while checking same-handle observations before and after.
    ///
    /// This detects observed identity, size, or native timestamp changes; it
    /// does not provide an atomic snapshot against adversarial concurrent I/O.
    /// Returns `ChangedDuringRead` when the observations do not match.
    pub fn read_consistent_with_options(
        &self,
        path: &Path,
        options: &LocalSnapshotOptions,
    ) -> LocalResult<crate::LocalFileSnapshot> {
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let reader = self.open_resolved_reader_with_policy(
            &resolved,
            options.read_options(),
            LocalFileOperation::Read,
            resolver.current_directory(),
            options.symlink_policy().unwrap_or(self.symlink_policy),
        )?;
        let mut observed =
            LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required)?;
        let before = observed.observation().clone();
        if before.metadata().len() > options.max_bytes() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ResourceLimit,
                LocalFileOperation::Read,
            )
            .with_path(resolved.namespace_absolute().to_path_buf()));
        }
        let bytes = observed.read_to_end_limited(options.max_bytes())?;
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        let after = observed.observe_now()?;
        if !crate::observation::snapshot::observations_match(&before, &after, bytes.len() as u64) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ChangedDuringRead,
                LocalFileOperation::Read,
            )
            .with_path(resolved.namespace_absolute().to_path_buf()));
        }
        Ok(crate::LocalFileSnapshot::new(bytes, before, after))
    }

    /// Reads final-entry metadata without following the final symlink.
    ///
    /// Returns resolution, policy, or native metadata errors. A
    /// directory-qualified operand returns `NotDirectory` unless the observed
    /// final entry is a real directory.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        self.core.fail_if_requested(TestFaultPoint::Metadata).map_err(|error| {
            with_current_directory(
                LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error),
                resolver.current_directory(),
            )
        })?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.metadata(resolved.authority_relative(), self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                .with_path(resolved.namespace_absolute().to_path_buf());
            return Err(with_current_directory(error, resolver.current_directory()));
        }
        Ok(metadata)
    }

    /// Opens a reader using this instance's default reader options.
    ///
    /// Returns the owned reader or error described by
    /// [`Self::open_reader_with_options`].
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    ///
    /// Requires a regular file after applying this instance's symlink policy.
    /// Returns path-resolution, type, or native open errors, including retry
    /// timeout failures. The returned reader owns the opened native handle.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
        self.open_resolved_reader(
            &resolved,
            options,
            LocalFileOperation::OpenReader,
            resolver.current_directory(),
        )
    }

    /// Opens an already-bound operand without querying the process PWD again.
    ///
    /// Returns an opened regular-file reader or a structured opening error
    /// carrying the same namespace path and PWD snapshot as the caller.
    fn open_resolved_reader(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
    ) -> LocalResult<LocalFileReader> {
        self.open_resolved_reader_with_policy(
            resolved,
            options,
            operation,
            current_directory,
            self.symlink_policy,
        )
    }

    fn open_resolved_reader_with_policy(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
        symlink_policy: crate::LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        reject_directory_qualified_file(resolved, operation, current_directory)?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_reader_with_policy(
                resolved.authority_relative(),
                options,
                symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_reader(resolved.authority_relative(), options, symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                operation,
                resolved.namespace_absolute(),
                None,
                current_directory,
            )
        })
    }

    /// Reads at most `max_bytes` using the default reader options.
    ///
    /// Returns the prefix or error described by
    /// [`Self::read_prefix_with_options`].
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>> {
        self.read_prefix_with_options(path, max_bytes, &self.defaults.read)
    }

    /// Reads at most `max_bytes` using one complete explicit options value.
    ///
    /// Stops at the byte limit or EOF and returns an owned byte vector. A zero
    /// limit still resolves and opens the source. Returns the opening errors
    /// of [`Self::open_reader_with_options`] or a subsequent read error;
    /// interrupted reads are retried and partial bytes are discarded on error.
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>> {
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let error_path = resolved.namespace_absolute().to_path_buf();
        let mut reader = self.open_resolved_reader(
            &resolved,
            options,
            LocalFileOperation::Read,
            resolver.current_directory(),
        )?;
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(max_bytes.min(8192));
        let mut buffer = [0_u8; 8192];
        while result.len() < max_bytes {
            let read_len = (max_bytes - result.len()).min(buffer.len());
            #[cfg(feature = "test-support")]
            if take_test_support("local-fs-read-prefix-read") {
                let error = LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(error_path),
                    None,
                    std::io::Error::other("injected prefix read failure"),
                );
                return Err(with_current_directory(error, resolver.current_directory()));
            }
            #[cfg(feature = "test-support")]
            let read_result = if take_test_support("local-prefix-interrupted") {
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            } else {
                Input::read(&mut reader, &mut buffer[..read_len])
            };
            #[cfg(not(feature = "test-support"))]
            let read_result = Input::read(&mut reader, &mut buffer[..read_len]);
            if matches!(&read_result, Err(error) if error.kind() == std::io::ErrorKind::Interrupted) {
                continue;
            }
            let count = read_result.map_err(|source| {
                with_current_directory(
                    LocalFileError::from_io(LocalFileOperation::Read, Some(error_path.clone()), None, source),
                    resolver.current_directory(),
                )
            })?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..count]);
        }
        Ok(result)
    }

    /// Opens a writer using this instance's default writer options.
    ///
    /// Returns the owned writer or error described by
    /// [`Self::open_writer_with_options`].
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter> {
        self.open_writer_with_options(path, &self.defaults.write)
    }

    /// Opens a writer using one complete explicit options value.
    ///
    /// Create modes return an unpublished staging writer; append opens an
    /// existing regular file directly. Returns path, type, requirement, or
    /// native preparation errors. Requested parent creation may remain after
    /// an error. Commit or abort a staging writer explicitly when its final
    /// publication or cleanup result must be observed.
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter> {
        validate_write_options(options, self.capabilities(), LocalFileOperation::OpenWriter).map_err(|error| {
            with_current_directory(
                error.with_path(path.to_path_buf()),
                self.current_directory.virtual_path(),
            )
        })?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_writer_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_writer(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map(|writer| {
            writer.bind_namespace(
                resolved.namespace_absolute().to_path_buf(),
                resolver.current_directory().map(Path::to_path_buf),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }
}
