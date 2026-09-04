// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::capability::LocalFileSystemCapabilities;
use qubit_local_files::capability::LocalFileSystemLimits;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileErrorSource;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalPathCodecError;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalRenameOptions;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::outcome::LocalCopyMethod;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalRenameFailureState;
use qubit_local_files::path::LocalFileNames;
use qubit_local_files::path::LocalPaths;
use tempfile::tempdir;

/// Verifies every capability accessor returns a coherent platform snapshot.
#[test]
fn test_capability_snapshot_exposes_all_guarantees() {
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let capabilities = std::hint::black_box(
        LocalFileSystem::capabilities as fn(&LocalFileSystem) -> LocalFileSystemCapabilities,
    )(&filesystem);

    let supports_rooted = std::hint::black_box(
        LocalFileSystemCapabilities::supports_rooted_operations as fn(LocalFileSystemCapabilities) -> bool,
    );
    assert!(supports_rooted(capabilities));
    let _ = capabilities.supports_atomic_rename();
    let atomic_replace = std::hint::black_box(
        LocalFileSystemCapabilities::supports_atomic_replace as fn(LocalFileSystemCapabilities) -> bool,
    );
    assert_eq!(
        cfg!(any(unix, windows)),
        std::hint::black_box(atomic_replace)(capabilities),
    );
    let _ = capabilities.can_attempt_atomic_temp_persist();
    let _ = capabilities.supports_durable_file_copy();
    let _ = capabilities.supports_durable_write();
    let _ = capabilities.supports_durable_temp_file_persist();

    let limits =
        std::hint::black_box(LocalFileSystem::limits as fn(&LocalFileSystem) -> LocalFileSystemLimits)(&filesystem);
    let max_path_length =
        std::hint::black_box(LocalFileSystemLimits::max_path_length as fn(&LocalFileSystemLimits) -> _);
    let max_component_length =
        std::hint::black_box(LocalFileSystemLimits::max_component_length as fn(&LocalFileSystemLimits) -> _);
    assert_eq!(max_path_length(&limits), max_component_length(&limits));
}

/// Verifies structured errors preserve each supported I/O classification and
/// all optional context fields.
#[test]
fn test_local_file_error_classifies_io_and_retains_context() {
    for (native, expected) in [
        (io::ErrorKind::NotFound, LocalFileErrorKind::NotFound),
        (io::ErrorKind::AlreadyExists, LocalFileErrorKind::AlreadyExists),
        (io::ErrorKind::NotADirectory, LocalFileErrorKind::NotDirectory),
        (io::ErrorKind::IsADirectory, LocalFileErrorKind::IsDirectory),
        (io::ErrorKind::PermissionDenied, LocalFileErrorKind::PermissionDenied),
        (io::ErrorKind::InvalidInput, LocalFileErrorKind::InvalidPath),
        (io::ErrorKind::InvalidData, LocalFileErrorKind::DataCorruption),
        (io::ErrorKind::Unsupported, LocalFileErrorKind::Unsupported),
        (io::ErrorKind::OutOfMemory, LocalFileErrorKind::ResourceLimit),
        (io::ErrorKind::StorageFull, LocalFileErrorKind::ResourceLimit),
        (io::ErrorKind::Other, LocalFileErrorKind::Io),
    ] {
        let error = LocalFileError::from_io(
            LocalFileOperation::Copy,
            Some("source".into()),
            Some("target".into()),
            io::Error::from(native),
        );

        assert_eq!(expected, error.kind());
        assert_eq!(Some(Path::new("source")), error.path());
        assert_eq!(Some(Path::new("target")), error.target());
        assert!(error.typed_source().is_some());
        assert!(Error::source(&error).is_some());
        assert!(error.to_string().contains("targeting target"));
        assert!(error.to_string().contains("caused by"));
    }

    let error = LocalFileError::new(LocalFileErrorKind::PublicationIncomplete, LocalFileOperation::Commit)
        .with_path("staging".into())
        .with_target("published".into());
    assert!(error.into_source().is_none());
}

/// Verifies both retained error-source variants preserve their display and
/// standard-error chaining behavior.
#[test]
fn test_local_file_error_sources_preserve_typed_causes() {
    let io_source = LocalFileErrorSource::Io(io::Error::from(io::ErrorKind::PermissionDenied));
    assert!(io_source.to_string().contains("permission denied"));
    assert!(Error::source(&io_source).is_some());

    let codec_source = LocalFileErrorSource::PathCodec(LocalPathCodecError::InvalidEscape { offset: 4 });
    assert!(codec_source.to_string().contains("4"));
    assert!(Error::source(&codec_source).is_some());
}

/// Verifies consuming an error preserves its typed source and that errors
/// without an originating cause retain no source.
#[test]
fn test_local_file_error_consumes_optional_typed_source() {
    let source = LocalFileError::from_io(
        LocalFileOperation::OpenReader,
        None,
        None,
        io::Error::from(io::ErrorKind::TimedOut),
    )
    .into_source()
    .expect("I/O construction should retain its source");
    assert!(matches!(source, LocalFileErrorSource::Io(error) if error.kind() == io::ErrorKind::TimedOut));

    let error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter);
    assert!(error.typed_source().is_none());
    assert!(Error::source(&error).is_none());
    assert_eq!("OpenWriter failed with RequirementNotMet", error.to_string());
    assert!(error.into_source().is_none());
}

/// Verifies formatting propagates a caller-owned output failure instead of
/// masking it behind the structured filesystem error.
#[test]
fn test_local_file_error_display_propagates_formatter_failure() {
    struct RejectingWriter {
        successful_writes: usize,
    }

    impl fmt::Write for RejectingWriter {
        fn write_str(&mut self, _value: &str) -> fmt::Result {
            if self.successful_writes == 0 {
                Err(fmt::Error)
            } else {
                self.successful_writes -= 1;
                Ok(())
            }
        }
    }

    for successful_writes in 0..16 {
        let error = LocalFileError::new(LocalFileErrorKind::Io, LocalFileOperation::Metadata)
            .with_path("source".into())
            .with_target("target".into());
        let mut writer = RejectingWriter { successful_writes };
        let _ = fmt::write(&mut writer, format_args!("{error}"));
    }
}

/// Verifies all public path-codec diagnostics produce stable, useful text.
#[test]
fn test_path_codec_error_formats_each_public_variant() {
    let cases = [
        (LocalPathCodecError::InvalidEscape { offset: 7 }, "7".to_owned()),
        (LocalPathCodecError::NonCanonicalText, "non-canonical".to_owned()),
        (LocalPathCodecError::NativeNul, "NUL".to_owned()),
        (LocalPathCodecError::UnsupportedNativeEncoding, "unsupported".to_owned()),
        (
            LocalPathCodecError::UnrepresentableNativeValue,
            "cannot represent".to_owned(),
        ),
    ];

    for (error, expected_text) in cases {
        assert!(error.to_string().contains(&expected_text));
        assert!(Error::source(&error).is_none());
    }
}

/// Verifies native filename helpers preserve platform components and reject
/// invalid portable input without requiring UTF-8 path conversion.
#[test]
fn test_native_file_name_helpers_cover_component_and_validation_paths() {
    let value = Path::new("archive.tar.gz");
    let names = LocalFileNames::portable();

    assert_eq!(Some(OsStr::new("archive.tar.gz")), value.file_name());
    assert_eq!(Some(OsStr::new("archive.tar")), value.file_stem());
    assert_eq!(Some(OsStr::new("archive")), value.file_prefix());
    assert_eq!(Some(OsStr::new("gz")), value.extension());
    assert!(names.validate(OsStr::new("safe-name.txt")).is_ok());
    assert!(names.validate(OsStr::new("bad/name")).is_err());
    assert!(
        names
            .random_name()
            .expect("a random native name should be generated")
            .to_string_lossy()
            .starts_with("qubit-local-files-")
    );
    let error = names
        .random_name_with(Some(OsStr::new("invalid/name")), None)
        .expect_err("path separators must be rejected in a random-name prefix");
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
    for invalid in ["bad\0name", "bad\\name", "../name"] {
        assert!(
            names.random_name_with(Some(OsStr::new(invalid)), None).is_err(),
            "expected random-name fragment to be rejected: {invalid:?}",
        );
    }
}

/// Verifies native names without UTF-8 text are rejected before portable-name
/// validation attempts to interpret their bytes.
#[cfg(unix)]
#[test]
fn test_native_file_name_validation_rejects_non_utf8_component() {
    use std::os::unix::ffi::OsStrExt;

    let error = LocalFileNames::portable()
        .validate(OsStr::from_bytes(b"\xff"))
        .expect_err("non-UTF-8 names cannot satisfy portable-text rules");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
}

/// Verifies rooted path objects retain the virtual-root representation.
#[test]
fn test_local_paths_cover_rooted_virtual_root() {
    assert!(LocalPaths::rooted().to_canonical_components(Path::new("/")).is_ok());
    assert!(LocalPaths::rooted().to_canonical_components(Path::new("")).is_err());
}

/// Verifies normalized metadata distinguishes empty files and directories,
/// and that the coverage-visible free metadata API reports missing entries.
#[test]
fn test_public_metadata_values_cover_file_directory_and_missing_cases() {
    let directory = tempdir().expect("temporary directory should be created");
    let empty_file = directory.path().join("empty");
    let child_directory = directory.path().join("child-directory");
    fs::write(&empty_file, []).expect("empty file should be written");
    fs::create_dir(&child_directory).expect("child directory should be created");

    let file_metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&empty_file)
        .expect("empty file metadata should be readable");
    assert_eq!(LocalFileKind::File, file_metadata.kind());
    assert!(file_metadata.is_empty());
    let directory_metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&child_directory)
        .expect("directory metadata should be readable");
    assert_eq!(LocalFileKind::Directory, directory_metadata.kind());

    assert_eq!(
        LocalFileErrorKind::NotFound,
        LocalFileSystem::host()
            .expect("Host filesystem should open")
            .metadata(&directory.path().join("missing"))
            .expect_err("missing metadata should fail")
            .kind(),
    );
}

/// Verifies metadata preserves a symlink's own type rather than normalizing
/// it as the type of its referent.
#[cfg(unix)]
#[test]
fn test_public_metadata_values_distinguish_symbolic_links() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let link = directory.path().join("link");
    fs::write(&referent, b"payload").expect("referent should be written");
    symlink(&referent, &link).expect("symbolic link should be created");

    let metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&link)
        .expect("symbolic link metadata should be readable");
    assert_eq!(LocalFileKind::Symlink, metadata.kind());
}

/// Verifies metadata classifies Unix-domain sockets explicitly.
#[cfg(unix)]
#[test]
fn test_public_metadata_values_classify_unix_socket() {
    use std::os::unix::net::UnixListener;

    let directory = tempdir().expect("temporary directory should be created");
    let socket = directory.path().join("socket");
    let _listener = match UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping Unix socket metadata classification: socket creation is not permitted");
            return;
        }
        Err(error) => panic!("Unix-domain socket should be created: {error}"),
    };

    let metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&socket)
        .expect("socket metadata should be readable");
    assert_eq!(LocalFileKind::Socket, metadata.kind());
}

/// Verifies metadata and operation outcomes expose both changed and no-op
/// states through their public APIs.
#[test]
fn test_metadata_and_outcomes_expose_public_values() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("file");
    let created_directory = directory.path().join("created");
    let renamed_file = directory.path().join("renamed");
    fs::write(&file, b"payload").expect("test file should be written");

    let metadata = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .metadata(&file)
        .expect("file metadata should be readable");
    assert_eq!(LocalFileKind::File, metadata.kind());
    assert_eq!(7, metadata.len());
    assert!(!metadata.is_empty());
    let _ = metadata.accessed_at();
    let _ = metadata.modified_at();
    let _ = metadata.created_at();

    let created = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&created_directory, &LocalCreateDirectoryOptions::new())
        .expect("directory should be created");
    assert!(created.created());
    let existing = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&created_directory, &LocalCreateDirectoryOptions::new().with_exists_ok())
        .expect("existing directory should be accepted");
    assert!(!existing.created());

    let renamed = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&file, &renamed_file, &LocalRenameOptions::new())
        .expect("file should be renamed");
    assert!(renamed.atomic());
    let _ = renamed.durable();

    let copied = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&renamed_file, &directory.path().join("copy"), &LocalCopyOptions::new())
        .expect("file should be copied");
    assert_eq!(LocalCopyMethod::StagedFile, copied.method());
    assert_eq!(1, copied.stats().files());
    assert_eq!(7, copied.stats().bytes());
    assert_eq!(0, copied.stats().directories());
    assert_eq!(0, copied.stats().skipped());
    assert_eq!(0, copied.stats().overwritten());
    assert!(copied.atomic());
    let _ = copied.durable();
    let _ = copied.metadata_preservation();

    let deleted = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_file_with_options(&renamed_file, &LocalDeleteOptions::new())
        .expect("file should be deleted");
    assert!(deleted.deleted());
    let missing = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_file_with_options(&renamed_file, &LocalDeleteOptions::new().with_missing_ok())
        .expect("missing file should be accepted");
    assert!(!missing.deleted());
}

/// Verifies recursive copy statistics distinguish new directories, skipped
/// files, and overwritten files.
#[test]
fn test_recursive_copy_outcome_reports_all_public_statistics() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("child"), b"new").expect("source child should be written");

    let initial = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&source, &target, &LocalCopyOptions::new().with_tree_source())
        .expect("directory should be copied");
    assert_eq!(LocalCopyMethod::Recursive, initial.method());
    assert_eq!(1, initial.stats().directories());
    assert_eq!(1, initial.stats().files());
    assert_eq!(3, initial.stats().bytes());

    let skipped = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Skip),
        )
        .expect("existing child should be skipped");
    assert_eq!(1, skipped.stats().skipped());

    let overwritten = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect("existing child should be overwritten");
    assert_eq!(1, overwritten.stats().overwritten());
}

/// Verifies public typed copy and rename failures expose diagnostic text,
/// causal chains, and all retained parts for unchanged preflight failures.
#[test]
fn test_public_failure_values_expose_display_sources_and_parts() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing = directory.path().join("missing");
    let target = directory.path().join("target");

    let copy_failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&missing, &target, &LocalCopyOptions::new())
        .expect_err("missing copy source should fail");
    assert!(copy_failure.to_string().contains("copy failed"));
    assert!(Error::source(&copy_failure).is_some());
    assert_eq!(LocalCopyFailureState::Unchanged, copy_failure.state());
    assert_eq!(LocalFileOperation::Copy, copy_failure.error().operation(),);
    assert_eq!(0, copy_failure.partial_stats().files());
    assert!(copy_failure.staging_path().is_none());
    assert!(copy_failure.cleanup_error().is_none());

    let rename_failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&missing, &target, &LocalRenameOptions::new())
        .expect_err("missing rename source should fail");
    assert!(rename_failure.to_string().contains("rename failed"));
    assert!(Error::source(&rename_failure).is_some());
    let (error, state) = rename_failure.into_parts();
    assert_eq!(LocalRenameFailureState::Unchanged, state);
    assert_eq!(LocalFileOperation::Rename, error.operation());
}
