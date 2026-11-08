// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.

use std::path::Path;

use qubit_io::ByteInput;
use qubit_io::Input;

use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalReadOptions;
use super::LocalResult;
use super::operation_error;
use super::resolve_operation_path;
use super::validate_scope_symlink_policy;
use super::with_current_directory;
#[cfg(feature = "test-support")]
use crate::local::take_test_support;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalObservedReader;
use crate::options::LocalReadAllOptions;
use crate::options::LocalSnapshotOptions;

impl LocalFileSystem {
    /// Reads a regular file completely using the filesystem's reader defaults.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the regular file to read.
    ///
    /// # Returns
    ///
    /// All bytes read from the file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, or reading errors.
    pub fn read_all(&self, path: &Path) -> LocalResult<Vec<u8>> {
        self.read_all_with_options(path, &LocalReadAllOptions::new())
    }
    /// Reads a regular file completely with an optional byte limit.
    ///
    /// Returns `ResourceLimit` when the observed length or bytes read exceed
    /// the configured maximum. Other open/read failures retain structured
    /// operation context; partial bytes are discarded.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the regular file to read.
    /// - `options`: Reader overrides and optional maximum byte count.
    ///
    /// # Returns
    ///
    /// All bytes read from the file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, reading, or byte-limit errors.
    pub fn read_all_with_options(&self, path: &Path, options: &LocalReadAllOptions) -> LocalResult<Vec<u8>> {
        self.read_all_with_context(path, options, |bytes, _, _| Ok(bytes))
    }
    /// Reads a complete UTF-8 text file using the filesystem's reader defaults.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the UTF-8 file to read.
    ///
    /// # Returns
    ///
    /// The file contents decoded as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, reading, or UTF-8 decoding errors.
    pub fn read_text(&self, path: &Path) -> LocalResult<String> {
        self.read_text_with_options(path, &LocalReadAllOptions::new())
    }
    /// Reads a complete UTF-8 text file with an optional byte limit.
    ///
    /// Returns `InvalidEncoding` with the original UTF-8 conversion error
    /// when the complete file is not valid UTF-8.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the UTF-8 file to read.
    /// - `options`: Reader overrides and optional maximum byte count.
    ///
    /// # Returns
    ///
    /// The file contents decoded as UTF-8.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, reading, byte-limit, or UTF-8 decoding
    /// errors.
    pub fn read_text_with_options(&self, path: &Path, options: &LocalReadAllOptions) -> LocalResult<String> {
        self.read_all_with_context(path, options, |bytes, absolute, pwd| {
            String::from_utf8(bytes).map_err(|source| {
                with_current_directory(
                    LocalFileError::from_utf8(LocalFileOperation::Read, Some(absolute.to_path_buf()), source),
                    pwd,
                )
            })
        })
    }
    /// Reads bytes while checking same-handle observations before and after.
    ///
    /// This detects observed identity, size, or native timestamp changes; it
    /// does not provide an atomic snapshot against adversarial concurrent I/O.
    /// Returns `ChangedDuringRead` when the observations do not match.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the file to observe and read.
    /// - `options`: Snapshot limits and reader policy.
    ///
    /// # Returns
    ///
    /// A snapshot containing the bytes and before/after observations.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, observation, byte-limit, or
    /// changed-during-read errors.
    pub fn read_consistent_with_options(
        &self,
        path: &Path,
        options: &LocalSnapshotOptions,
    ) -> LocalResult<crate::LocalFileSnapshot> {
        let symlink_policy = self.read_symlink_policy(options.read_options(), options.symlink_policy());
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let reader = self.open_resolved_reader_with_policy(
            &resolved,
            options.read_options(),
            LocalFileOperation::Read,
            resolver.current_directory(),
            symlink_policy,
        )?;
        let mut observed = LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required)
            .map_err(|error| {
                operation_error(
                    error,
                    LocalFileOperation::Read,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                )
            })?
            .bind_error_context(
                resolved.namespace_absolute().to_path_buf(),
                resolver.current_directory().map(Path::to_path_buf),
            );
        let before = observed.observation().clone();
        if before.metadata().len() > options.max_bytes() {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        let bytes = observed.read_to_end_limited(options.max_bytes()).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        let after = observed.observe_now().map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if !crate::observation::snapshot::observations_match(&before, &after, bytes.len() as u64) {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        Ok(crate::LocalFileSnapshot::new(bytes, before, after))
    }
    /// Reads at most `max_bytes` using the default reader options.
    ///
    /// Returns the prefix or error described by
    /// [`Self::read_prefix_with_options`].
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the file to read.
    /// - `max_bytes`: Maximum number of bytes to return.
    ///
    /// # Returns
    ///
    /// At most `max_bytes` from the beginning of the file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, reading, or allocation-limit errors.
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>> {
        self.read_prefix_with_options(path, max_bytes, &self.defaults.read)
    }
    /// Reads at most `max_bytes` using one complete explicit options value.
    ///
    /// Stops at the byte limit or EOF and returns an owned byte vector. A zero
    /// limit still resolves and opens the source. Returns the opening errors
    /// of [`Self::open_reader_with_options`] or a subsequent read error;
    /// interrupted reads are retried and partial bytes are discarded on error.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the file to read.
    /// - `max_bytes`: Maximum number of bytes to return.
    /// - `options`: Complete reader policy for this operation.
    ///
    /// # Returns
    ///
    /// At most `max_bytes` from the beginning of the file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, reading, or allocation-limit errors.
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>> {
        let symlink_policy = self.read_symlink_policy(options, None);
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
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
        collect_prefix_with_reserver(&mut reader, max_bytes, |output, additional| {
            try_reserve_prefix(output, additional)
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                &error_path,
                None,
                resolver.current_directory(),
            )
        })
    }
    /// Reads the file and passes its bytes and resolved path context to
    /// `finish`.
    ///
    /// The callback runs only after the complete bounded read succeeds and can
    /// preserve namespace and PWD context when converting the result.
    ///
    /// # Type Parameters
    ///
    /// - `T`: Result type produced by the conversion callback.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the regular file to read.
    /// - `options`: Reader overrides and optional maximum byte count.
    /// - `finish`: Conversion applied to the complete bytes and resolved
    ///   context.
    fn read_all_with_context<T>(
        &self,
        path: &Path,
        options: &LocalReadAllOptions,
        finish: impl FnOnce(Vec<u8>, &Path, Option<&Path>) -> LocalResult<T>,
    ) -> LocalResult<T> {
        let read_options = options.read_options().unwrap_or(&self.defaults.read);
        let symlink_policy = self.read_symlink_policy(read_options, options.symlink_policy());
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let mut reader = self.open_resolved_reader_with_policy(
            &resolved,
            read_options,
            LocalFileOperation::Read,
            resolver.current_directory(),
            symlink_policy,
        )?;
        let maximum = options.max_bytes();
        if maximum.is_some_and(|limit| reader.metadata().len() > limit) {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        let bytes = crate::observation::reader::collect_limited(&mut reader, maximum).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        finish(bytes, resolved.namespace_absolute(), resolver.current_directory())
    }
}

/// Reserves prefix-buffer capacity and maps allocation failure through an I/O
/// error.
///
/// # Parameters
///
/// - `output`: Buffer whose capacity is increased.
/// - `additional`: Additional capacity requested.
///
/// # Errors
///
/// Returns an I/O error when the requested capacity cannot be reserved.
fn try_reserve_prefix(output: &mut Vec<u8>, additional: usize) -> std::io::Result<()> {
    #[cfg(test)]
    if tests::should_fail_prefix_reserve() {
        return Err(std::io::Error::other("injected prefix reserve failure"));
    }
    output.try_reserve(additional).map_err(std::io::Error::other)
}
/// Collects at most `maximum` bytes, retrying interrupted reads and discarding
/// partial output on error.
///
/// # Type Parameters
///
/// - `S`: Byte input supplying the prefix data.
/// - `F`: Reservation strategy invoked before bytes are appended.
///
/// # Parameters
///
/// - `input`: Input stream to read.
/// - `maximum`: Maximum number of bytes to collect.
/// - `reserve`: Capacity reservation strategy used before extending the output.
///
/// # Returns
///
/// The collected prefix, up to `maximum` bytes.
///
/// # Errors
///
/// Returns `ResourceLimit` when reservation fails, or an I/O error when reading
/// fails.
fn collect_prefix_with_reserver<S, F>(input: &mut S, maximum: usize, mut reserve: F) -> LocalResult<Vec<u8>>
where
    S: ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> std::io::Result<()>,
{
    let mut result = Vec::new();
    let mut buffer = [0_u8; 8192];
    while result.len() < maximum {
        let read_len = (maximum - result.len()).min(buffer.len());
        #[cfg(feature = "test-support")]
        if take_test_support("local-fs-read-prefix-read") {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Read,
                None,
                None,
                std::io::Error::other("injected prefix read failure"),
            ));
        }
        #[cfg(feature = "test-support")]
        let read_result = if take_test_support("local-prefix-interrupted") {
            Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
        } else {
            Input::read(input, &mut buffer[..read_len])
        };
        #[cfg(not(feature = "test-support"))]
        let read_result = Input::read(input, &mut buffer[..read_len]);
        if matches!(&read_result, Err(error) if error.kind() == std::io::ErrorKind::Interrupted) {
            continue;
        }
        let count =
            read_result.map_err(|source| LocalFileError::from_io(LocalFileOperation::Read, None, None, source))?;
        if count == 0 {
            break;
        }
        reserve(&mut result, count)
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read))?;
        result.extend_from_slice(&buffer[..count]);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;
    use std::path::Path;

    use super::super::LocalFileSystem;
    use super::super::operation_error;
    use super::collect_prefix_with_reserver;
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileErrorSource;
    use crate::error::LocalFileOperation;

    thread_local! {
        static PREFIX_RESERVE_FAILURE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(super) fn should_fail_prefix_reserve() -> bool {
        PREFIX_RESERVE_FAILURE_AFTER.with(|failure_after| match failure_after.get() {
            Some(0) => {
                failure_after.set(None);
                true
            }
            Some(remaining) => {
                failure_after.set(Some(remaining - 1));
                false
            }
            None => false,
        })
    }

    fn fail_prefix_reserve_after(successful_reserves: usize) {
        PREFIX_RESERVE_FAILURE_AFTER.with(|failure_after| failure_after.set(Some(successful_reserves)));
    }

    fn assert_prefix_allocation_context(
        filesystem: &LocalFileSystem,
        path: &Path,
        expected_path: &Path,
        expected_current_directory: Option<&Path>,
    ) {
        fail_prefix_reserve_after(0);
        let error = filesystem
            .read_prefix(path, 8)
            .expect_err("injected reserve failure should fail the facade read");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Read);
        assert_eq!(error.path(), Some(expected_path));
        assert_eq!(error.current_directory(), expected_current_directory);
    }

    #[test]
    fn test_prefix_allocation_failure_keeps_host_absolute_context() {
        let directory = tempfile::tempdir().expect("fixture should exist");
        let path = directory.path().join("file");
        std::fs::write(&path, b"payload").expect("file should exist");
        let filesystem = LocalFileSystem::host().expect("Host should open");
        assert_prefix_allocation_context(&filesystem, &path, &path, None);
    }

    #[test]
    fn test_prefix_allocation_failure_keeps_host_relative_context() {
        if std::env::var_os("QUBIT_PREFIX_RELATIVE_CHILD").is_some() {
            let filesystem = LocalFileSystem::host().expect("Host should open");
            let directory = std::env::current_dir().expect("child PWD should be readable");
            assert_prefix_allocation_context(
                &filesystem,
                Path::new("file"),
                &directory.join("file"),
                Some(&directory),
            );
            return;
        }

        let directory = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(directory.path().join("file"), b"payload").expect("file should exist");
        let output = std::process::Command::new(std::env::current_exe().expect("test executable should exist"))
            .current_dir(directory.path())
            .env("QUBIT_PREFIX_RELATIVE_CHILD", "1")
            .args([
                "--exact",
                "local_file_system::content_read::tests::test_prefix_allocation_failure_keeps_host_relative_context",
                "--nocapture",
            ])
            .output()
            .expect("isolated relative-path test process should run");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success()
                && stdout.contains("running 1 test")
                && stdout.contains("test_prefix_allocation_failure_keeps_host_relative_context ... ok"),
            "isolated relative-path test must execute and pass; status: {}; stdout: {}; stderr: {}",
            output.status,
            stdout,
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[test]
    fn test_prefix_allocation_failure_keeps_rooted_virtual_context() {
        let directory = tempfile::tempdir().expect("fixture should exist");
        std::fs::write(directory.path().join("file"), b"payload").expect("file should exist");
        let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted should open");
        assert_prefix_allocation_context(
            &filesystem,
            Path::new("/file"),
            Path::new("/file"),
            Some(Path::new("/")),
        );
    }

    #[test]
    fn test_prefix_allocation_failure_before_first_chunk_is_resource_limit() {
        let mut input = Cursor::new(b"payload");
        let error = collect_prefix_with_reserver(&mut input, 8, |_, _| Err(std::io::Error::other("reserve failure")))
            .expect_err("first reserve should fail");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(error.operation(), LocalFileOperation::Read);
        let error = operation_error(
            error,
            LocalFileOperation::Read,
            Path::new("/resolved/file"),
            None,
            Some(Path::new("/captured/pwd")),
        );
        assert_eq!(error.path(), Some(Path::new("/resolved/file")));
        assert_eq!(error.current_directory(), Some(Path::new("/captured/pwd")));
    }

    #[test]
    fn test_prefix_allocation_failure_after_chunk_discards_partial_bytes() {
        let mut input = Cursor::new(vec![b'x'; 8193]);
        let mut reserves = 0;
        let error = collect_prefix_with_reserver(&mut input, 8193, |output, additional| {
            reserves += 1;
            if reserves == 1 {
                output.try_reserve(additional).map_err(std::io::Error::other)
            } else {
                Err(std::io::Error::other("reserve failure"))
            }
        })
        .expect_err("later reserve should fail");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    }

    #[test]
    fn test_prefix_collection_preserves_eof_limit_short_read_and_interrupted_behavior() {
        struct ShortReader {
            input: Cursor<&'static [u8]>,
            interrupt_next: bool,
        }

        impl std::io::Read for ShortReader {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                if self.interrupt_next {
                    self.interrupt_next = false;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                let limit = output.len().min(2);
                std::io::Read::read(&mut self.input, &mut output[..limit])
            }
        }

        let mut input = ShortReader {
            input: Cursor::new(b"payload"),
            interrupt_next: true,
        };
        let result = collect_prefix_with_reserver(&mut input, 7, |output, additional| {
            output.try_reserve(additional).map_err(std::io::Error::other)
        })
        .expect("prefix should collect");
        assert_eq!(result, b"payload");
    }

    #[test]
    fn test_read_operation_context_preserves_typed_observation_source() {
        let error = LocalFileError::from_io(
            LocalFileOperation::Metadata,
            Some(Path::new("native-path").to_path_buf()),
            None,
            std::io::Error::other("observation-source-sentinel"),
        );

        let wrapped = operation_error(
            error,
            LocalFileOperation::Read,
            Path::new("/sub/file"),
            None,
            Some(Path::new("/sub")),
        );

        assert_eq!(wrapped.kind(), LocalFileErrorKind::Io);
        assert_eq!(wrapped.operation(), LocalFileOperation::Read);
        assert_eq!(wrapped.path(), Some(Path::new("/sub/file")));
        assert_eq!(wrapped.current_directory(), Some(Path::new("/sub")));
        assert!(matches!(
            wrapped.typed_source(),
            Some(LocalFileErrorSource::Io(source))
                if source.kind() == std::io::ErrorKind::Other
                    && source.to_string() == "observation-source-sentinel"
        ));

        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("text.txt");
        std::fs::write(&path, b"payload").expect("text fixture");
        let filesystem = LocalFileSystem::host().expect("host filesystem");
        let fault = crate::tests::native_io_fault_tests::NativeIoFaultGuard::fail_on(
            crate::tests::native_io_fault_tests::NativeIoPoint::Read,
            1,
        );
        let error = filesystem
            .read_text_with_options(&path, &crate::options::LocalReadAllOptions::new())
            .expect_err("a native read failure should retain the read operation context");
        assert!(fault.fired(), "the native read fault should be consumed");
        assert_eq!(LocalFileOperation::Read, error.operation());
        assert_eq!(Some(path.as_path()), error.path());
        assert_eq!(std::io::ErrorKind::Other, error.io_error_kind());
        assert!(error.to_string().contains("injected native I/O contract failure"));
    }
}
