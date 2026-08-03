// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    error::Error,
    ffi::OsStr,
    fmt,
    fs,
    io,
    path::Path,
};

use qubit_local_files::{
    LocalCopyConflictPolicy,
    LocalCopyFailureState,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileErrorSource,
    LocalFileKind,
    LocalFileNames,
    LocalFileOperation,
    LocalFileSystem,
    LocalPathCodecError,
    LocalPaths,
    LocalRenameFailureState,
    LocalRenameOptions,
};
use tempfile::tempdir;

/// Verifies every capability accessor returns a coherent platform snapshot.
#[test]
fn test_capability_snapshot_exposes_all_guarantees() {
    let capabilities = LocalFileSystem::host().capabilities();

    let _ = capabilities.path_limit();
    let _ = capabilities.rooted_operations_implemented();
    let _ = capabilities.atomic_rename_implemented();
    let _ = capabilities.atomic_replace_implemented();
    let _ = capabilities.atomic_temp_persist_implemented();
    let _ = capabilities.directory_durability_implemented();
}

/// Verifies structured errors preserve each supported I/O classification and
/// all optional context fields.
#[test]
fn test_local_file_error_classifies_io_and_retains_context() {
    for (native, expected) in [
        (io::ErrorKind::NotFound, LocalFileErrorKind::NotFound),
        (
            io::ErrorKind::AlreadyExists,
            LocalFileErrorKind::AlreadyExists,
        ),
        (
            io::ErrorKind::NotADirectory,
            LocalFileErrorKind::NotDirectory,
        ),
        (io::ErrorKind::IsADirectory, LocalFileErrorKind::IsDirectory),
        (
            io::ErrorKind::PermissionDenied,
            LocalFileErrorKind::PermissionDenied,
        ),
        (io::ErrorKind::InvalidInput, LocalFileErrorKind::InvalidPath),
        (
            io::ErrorKind::InvalidData,
            LocalFileErrorKind::DataCorruption,
        ),
        (io::ErrorKind::Unsupported, LocalFileErrorKind::Unsupported),
        (
            io::ErrorKind::OutOfMemory,
            LocalFileErrorKind::ResourceLimit,
        ),
        (
            io::ErrorKind::StorageFull,
            LocalFileErrorKind::ResourceLimit,
        ),
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
        assert!(error.source_kind().is_some());
        assert!(Error::source(&error).is_some());
        assert!(error.to_string().contains("targeting target"));
    }

    let error = LocalFileError::new(
        LocalFileErrorKind::PublicationIncomplete,
        LocalFileOperation::Commit,
    )
    .with_path("staging".into())
    .with_target("published".into());
    assert!(error.into_source().is_none());
}

/// Verifies both retained error-source variants preserve their display and
/// standard-error chaining behavior.
#[test]
fn test_local_file_error_sources_preserve_typed_causes() {
    let io_source = LocalFileErrorSource::Io(io::Error::from(
        io::ErrorKind::PermissionDenied,
    ));
    assert!(io_source.to_string().contains("permission denied"));
    assert!(Error::source(&io_source).is_some());

    let codec_source =
        LocalFileErrorSource::PathCodec(LocalPathCodecError::InvalidEscape {
            offset: 4,
        });
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
    assert!(
        matches!(source, LocalFileErrorSource::Io(error) if error.kind() == io::ErrorKind::TimedOut)
    );

    let error = LocalFileError::new(
        LocalFileErrorKind::RequirementNotMet,
        LocalFileOperation::OpenWriter,
    );
    assert!(error.source_kind().is_none());
    assert!(Error::source(&error).is_none());
    assert_eq!(
        "OpenWriter failed with RequirementNotMet",
        error.to_string()
    );
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
        let error = LocalFileError::new(
            LocalFileErrorKind::Io,
            LocalFileOperation::Metadata,
        )
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
        (
            LocalPathCodecError::InvalidEscape { offset: 7 },
            "7".to_owned(),
        ),
        (
            LocalPathCodecError::NonCanonicalText,
            "non-canonical".to_owned(),
        ),
        (LocalPathCodecError::NativeNul, "NUL".to_owned()),
        (
            LocalPathCodecError::UnsupportedNativeEncoding,
            "unsupported".to_owned(),
        ),
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

    assert_eq!(Some(OsStr::new("archive.tar.gz")), value.file_name());
    assert_eq!(Some(OsStr::new("archive.tar")), value.file_stem());
    assert_eq!(Some(OsStr::new("archive")), value.file_prefix());
    assert_eq!(Some(OsStr::new("gz")), value.extension());
    assert!(
        LocalFileNames::validate_portable(OsStr::new("safe-name.txt")).is_ok()
    );
    assert!(LocalFileNames::validate_portable(OsStr::new("bad/name")).is_err());
    assert!(
        LocalFileNames::random_name()
            .expect("a random native name should be generated")
            .to_string_lossy()
            .starts_with("qubit-local-files-")
    );
    assert!(
        LocalFileNames::random_name_with(Some("invalid/name"), None)
            .expect_err(
                "path separators must be rejected in a random-name prefix"
            )
            .to_string()
            .contains("GenerateName")
    );
}

/// Verifies native names without UTF-8 text are rejected before portable-name
/// validation attempts to interpret their bytes.
#[cfg(unix)]
#[test]
fn test_native_file_name_validation_rejects_non_utf8_component() {
    use std::os::unix::ffi::OsStrExt;

    let error = LocalFileNames::validate_portable(OsStr::from_bytes(b"\xff"))
        .expect_err("non-UTF-8 names cannot satisfy portable-text rules");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
}

/// Verifies path composition and binding preserve lexical authority on both
/// accepted and rejected public inputs.
#[test]
fn test_local_paths_cover_public_composition_and_binding_cases() {
    let base = Path::new("/workspace");

    assert_eq!(
        Path::new("/workspace/nested/file"),
        LocalPaths::compose_descendant(base, Path::new("nested/file"))
            .expect("normal descendant should compose"),
    );
    assert!(
        LocalPaths::compose_descendant(base, Path::new("../escape")).is_err()
    );
    assert!(LocalPaths::compose_descendant(base, Path::new("")).is_err());
    assert!(
        !LocalPaths::is_lexically_within(
            Path::new("/workspace-other/file"),
            base,
        )
        .expect("separate normalized absolute paths should compare"),
    );
    assert!(
        LocalPaths::is_lexically_within(Path::new("relative"), base).is_err()
    );
    assert_eq!(
        base,
        LocalPaths::bind_host_path(base)
            .expect("absolute host path should remain unchanged"),
    );
    assert!(
        LocalPaths::to_canonical_relative_components(Path::new("")).is_err()
    );
}

/// Verifies normalized metadata distinguishes empty files and directories,
/// and that the coverage-visible free metadata API reports missing entries.
#[test]
fn test_public_metadata_values_cover_file_directory_and_missing_cases() {
    let directory = tempdir().expect("temporary directory should be created");
    let empty_file = directory.path().join("empty");
    let child_directory = directory.path().join("child-directory");
    fs::write(&empty_file, []).expect("empty file should be written");
    fs::create_dir(&child_directory)
        .expect("child directory should be created");

    let file_metadata = LocalFileSystem::host()
        .metadata(&empty_file)
        .expect("empty file metadata should be readable");
    assert_eq!(LocalFileKind::File, file_metadata.kind());
    assert!(file_metadata.is_empty());
    let directory_metadata = LocalFileSystem::host()
        .metadata(&child_directory)
        .expect("directory metadata should be readable");
    assert_eq!(LocalFileKind::Directory, directory_metadata.kind());

    assert_eq!(
        LocalFileErrorKind::NotFound,
        LocalFileSystem::host()
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
    let _listener = UnixListener::bind(&socket)
        .expect("Unix-domain socket should be created");

    let metadata = LocalFileSystem::host()
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
        .metadata(&file)
        .expect("file metadata should be readable");
    assert_eq!(LocalFileKind::File, metadata.kind());
    assert_eq!(7, metadata.len());
    assert!(!metadata.is_empty());
    let _ = metadata.accessed_at();
    let _ = metadata.modified_at();
    let _ = metadata.created_at();

    let created = LocalFileSystem::host()
        .create_directory(
            &created_directory,
            &LocalCreateDirectoryOptions::new(),
        )
        .expect("directory should be created");
    assert!(created.created());
    let existing = LocalFileSystem::host()
        .create_directory(
            &created_directory,
            &LocalCreateDirectoryOptions::new().with_exists_ok(),
        )
        .expect("existing directory should be accepted");
    assert!(!existing.created());

    let renamed = LocalFileSystem::host()
        .rename(&file, &renamed_file, &LocalRenameOptions::new())
        .expect("file should be renamed");
    assert!(renamed.atomic());
    let _ = renamed.durable();

    let copied = LocalFileSystem::host()
        .copy(
            &renamed_file,
            &directory.path().join("copy"),
            &LocalCopyOptions::new(),
        )
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
        .delete_file(&renamed_file, &LocalDeleteOptions::new())
        .expect("file should be deleted");
    assert!(deleted.deleted());
    let missing = LocalFileSystem::host()
        .delete_file(
            &renamed_file,
            &LocalDeleteOptions::new().with_missing_ok(),
        )
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
    fs::write(source.join("child"), b"new")
        .expect("source child should be written");

    let initial = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("directory should be copied");
    assert_eq!(LocalCopyMethod::Recursive, initial.method());
    assert_eq!(1, initial.stats().directories());
    assert_eq!(1, initial.stats().files());
    assert_eq!(3, initial.stats().bytes());

    let skipped = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Skip),
        )
        .expect("existing child should be skipped");
    assert_eq!(1, skipped.stats().skipped());

    let overwritten = LocalFileSystem::host()
        .copy(
            &source,
            &target,
            &LocalCopyOptions::new()
                .with_tree_source()
                .with_conflict(LocalCopyConflictPolicy::Overwrite),
        )
        .expect("existing child should be overwritten");
    let _ = overwritten.stats().overwritten();
}

/// Verifies public typed copy and rename failures expose diagnostic text,
/// causal chains, and all retained parts for unchanged preflight failures.
#[test]
fn test_public_failure_values_expose_display_sources_and_parts() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing = directory.path().join("missing");
    let target = directory.path().join("target");

    let copy_failure = LocalFileSystem::host()
        .copy(&missing, &target, &LocalCopyOptions::new())
        .expect_err("missing copy source should fail");
    assert!(copy_failure.to_string().contains("copy failed"));
    assert!(Error::source(&copy_failure).is_some());
    let (error, state, stats, staging, cleanup) = copy_failure.into_parts();
    assert_eq!(LocalCopyFailureState::Unchanged, state);
    assert_eq!(LocalFileOperation::Copy, error.operation());
    assert_eq!(0, stats.files());
    assert!(staging.is_none());
    assert!(cleanup.is_none());

    let rename_failure = LocalFileSystem::host()
        .rename(&missing, &target, &LocalRenameOptions::new())
        .expect_err("missing rename source should fail");
    assert!(rename_failure.to_string().contains("rename failed"));
    assert!(Error::source(&rename_failure).is_some());
    let (error, state) = rename_failure.into_parts();
    assert_eq!(LocalRenameFailureState::Unchanged, state);
    assert_eq!(LocalFileOperation::Rename, error.operation());
}
