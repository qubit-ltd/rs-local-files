// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host operands retain native traversal order; Rooted operands remain lexical.

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;

/// Returned listing paths retain the requested spelling, including macOS's
/// canonical /private/var prefix rather than an invented /var alias.
#[test]
fn test_host_listing_preserves_explicit_canonical_root_spelling() {
    let fixture = tempfile::tempdir().expect("listing spelling fixture");
    let root = fs::canonicalize(fixture.path()).expect("canonical fixture path");
    fs::write(root.join("entry"), b"entry").expect("listing fixture entry");
    let host = LocalFileSystem::host().expect("Host filesystem");
    let mut walker = host.list(&root).expect("open explicit listing root");
    assert_eq!(walker.root(), root);
    let entry = walker.next().expect("one entry").expect("read entry");
    assert_eq!(entry.path(), root.join("entry"));
    assert!(walker.next().is_none());
}

/// Staging and installation accept the same long ordinary paths as std.
#[cfg(windows)]
#[test]
fn test_host_windows_long_path_staged_publication_matches_std() {
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;

    use qubit_local_files::options::LocalWriteMetadataPolicy;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;

    let fixture = tempfile::tempdir().expect("long-path fixture");
    let mut parent = fixture.path().to_path_buf();
    while parent.as_os_str().encode_wide().count() < 280 {
        parent.push("long-native-directory-component");
    }
    fs::create_dir_all(&parent).expect("std creates long parent");
    let path = parent.join("manifest.json");
    let host = LocalFileSystem::host().expect("Host filesystem");
    for policy in [
        LocalWriteMetadataPolicy::PreserveExisting,
        LocalWriteMetadataPolicy::UseStaging,
    ] {
        for existing in [false, true] {
            if existing {
                fs::write(&path, b"old").expect("std creates long target");
            }
            let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy);
            let mut writer = host
                .open_writer_with_options(&path, &options)
                .expect("open long-path writer");
            writer.write_all(b"new").expect("write staged bytes");
            let outcome = writer.commit().expect("publish long-path staging");
            assert!(outcome.atomic());
            assert_eq!(fs::read(&path).expect("std reads published target"), b"new");
            fs::remove_file(&path).expect("reset long target");
        }
    }
}

/// Explicit parent creation preserves native missing/.. side effects and the
/// reached destination.
#[cfg(unix)]
#[test]
fn test_host_parent_creation_keeps_native_dot_traversal_effects() {
    use std::io::Write;

    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;

    let fixture = tempfile::tempdir().expect("isolated native-parent fixture");
    let native_root = fixture.path().join("native");
    let library_root = fixture.path().join("library");
    fs::create_dir(&native_root).expect("native fixture root");
    fs::create_dir(&library_root).expect("library fixture root");
    for root in [&native_root, &library_root] {
        fs::write(root.join("existing"), b"old").expect("existing destination");
    }
    let native_path = native_root.join("missing/../existing");
    fs::create_dir_all(native_path.parent().expect("native parent")).expect("native parent creation");
    fs::write(&native_path, b"new").expect("native replacement");

    let host = LocalFileSystem::host().expect("Host filesystem");
    let path = library_root.join("missing/../existing");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_create_parent();
    let mut writer = host
        .open_writer_with_options(&path, &options)
        .expect("create native parents");
    writer.write_all(b"new").expect("staged replacement");
    let _ = writer.commit().expect("publish reached destination");
    assert_eq!(
        fs::read(library_root.join("existing")).expect("library destination"),
        fs::read(native_root.join("existing")).expect("native destination")
    );
    assert!(native_root.join("missing").is_dir());
    assert!(library_root.join("missing").is_dir());

    let parent = library_root.join("temp-parent/..");
    let mut temporary = host
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent).with_create_parent())
        .expect("temporary native parent creation");
    assert!(library_root.join("temp-parent").is_dir());
    temporary.cleanup().expect("explicit temporary cleanup");
    assert!(
        library_root.join("temp-parent").is_dir(),
        "caller parent effects outlive temporary cleanup"
    );
}
#[cfg(unix)]
use qubit_local_files::policy::LocalSymlinkPolicy;

/// Native parent traversal follows a preceding symlink before walking upward.
#[cfg(unix)]
#[test]
fn test_host_parent_follows_native_symlink_order() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("fixture directory");
    fs::create_dir_all(dir.path().join("a")).expect("a directory");
    fs::create_dir_all(dir.path().join("b/inner")).expect("b directory");
    fs::write(dir.path().join("a/config"), b"A").expect("a config");
    fs::write(dir.path().join("b/config"), b"B").expect("b config");
    symlink("../b/inner", dir.path().join("a/link")).expect("symlink fixture");
    let path = dir.path().join("a/link/../config");
    let host = LocalFileSystem::host().expect("host filesystem");
    assert_eq!(
        host.read_prefix(&path, 8).expect("native read"),
        fs::read(&path).expect("std read")
    );
    assert_eq!(host.read_prefix(&path, 8).expect("native read"), b"B");
    let rooted = LocalFileSystem::rooted(dir.path()).expect("rooted filesystem");
    assert_eq!(
        rooted
            .read_prefix(Path::new("a/link/../config"), 8)
            .expect("lexical read"),
        b"A"
    );
}

/// Missing or non-directory components cannot disappear during Host binding.
#[test]
fn test_host_does_not_erase_invalid_intermediate_components() {
    let dir = tempfile::tempdir().expect("fixture directory");
    fs::write(dir.path().join("config"), b"present").expect("config fixture");
    fs::write(dir.path().join("file"), b"file").expect("file fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    for input in ["missing/../config", "file/../config", "file/./child"] {
        let path = dir.path().join(input);
        // Windows native normalization differs, so use the local native oracle.
        let expected = fs::read(&path);
        let observed = host.read_prefix(&path, 32);
        match expected {
            Ok(bytes) => assert_eq!(observed.expect("native success"), bytes),
            Err(_) => assert!(observed.is_err(), "invalid component was erased: {input}"),
        }
    }
}

/// A rejected link must be inspected even when followed by a parent component.
#[cfg(unix)]
#[test]
fn test_host_reject_checks_traversed_link_before_parent() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("fixture directory");
    fs::create_dir(dir.path().join("inner")).expect("inner directory");
    fs::write(dir.path().join("config"), b"data").expect("config fixture");
    symlink("inner", dir.path().join("link")).expect("symlink fixture");
    let mut host = LocalFileSystem::host().expect("host filesystem");
    host.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject policy");
    assert!(host.read_prefix(&dir.path().join("link/../config"), 8).is_err());
}

/// Host root-parent traversal follows the platform root semantics.
#[cfg(unix)]
#[test]
fn test_host_root_parent_is_not_a_rooted_escape() {
    let host = LocalFileSystem::host().expect("host filesystem");
    assert!(host.metadata(Path::new("/..")).is_ok());
}

/// Metadata preserves dots, parent traversal and final native directory intent.
#[test]
fn test_host_metadata_directory_syntax_matches_native() {
    let dir = tempfile::tempdir().expect("fixture directory");
    fs::create_dir(dir.path().join("inner")).expect("inner directory");
    fs::write(dir.path().join("file"), b"data").expect("file fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    for input in [
        "inner/",
        "inner/.",
        "inner/..",
        "file/",
        "file/.",
        "file/..",
        "missing/..",
    ] {
        let path = dir.path().join(input);
        let expected = fs::symlink_metadata(&path);
        let observed = host.metadata(&path);
        match expected {
            Ok(metadata) if !metadata.is_dir() => {
                // Windows may normalize file/. to the file. The facade still
                // enforces its explicit directory-qualified operand contract.
                assert_eq!(
                    observed.expect_err("directory-qualified file must fail").kind(),
                    LocalFileErrorKind::NotDirectory
                );
            }
            Ok(metadata) => assert_eq!(observed.expect("native metadata").len(), metadata.len()),
            Err(_) => assert!(observed.is_err(), "invalid native operand: {input}"),
        }
    }
}

/// Dangling links and loops before a parent cannot collapse into valid files.
#[cfg(unix)]
#[test]
fn test_host_parent_does_not_hide_dangling_or_looping_links() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("fixture directory");
    fs::write(dir.path().join("config"), b"data").expect("config fixture");
    symlink("missing", dir.path().join("dangling")).expect("dangling link");
    symlink("loop", dir.path().join("loop")).expect("looping link");
    let host = LocalFileSystem::host().expect("host filesystem");
    for input in ["dangling/../config", "loop/../config"] {
        let path = dir.path().join(input);
        assert!(fs::read(&path).is_err());
        assert!(host.read_prefix(&path, 8).is_err());
        assert!(host.metadata(&path).is_err());
    }
}

/// Namespace binding preserves native bytes instead of collecting components.
#[cfg(unix)]
#[test]
fn test_host_binding_preserves_raw_spelling_and_non_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    use qubit_local_files::path::LocalFileSystemScope;
    use qubit_local_files::path::LocalPathResolver;
    let resolver = LocalPathResolver::new(LocalFileSystemScope::Host, Path::new("/base")).expect("resolver");
    let input = Path::new(OsStr::from_bytes(b"raw/./\xff/../leaf/"));
    let bound = resolver.resolve(input).expect("raw relative binding");
    assert_eq!(
        bound.authority_relative().as_os_str().as_bytes(),
        b"/base/raw/./\xff/../leaf/"
    );
    let absolute = Path::new(OsStr::from_bytes(b"/raw/./\xff/../leaf/"));
    assert_eq!(
        resolver
            .resolve(absolute)
            .expect("absolute binding")
            .authority_relative()
            .as_os_str(),
        absolute.as_os_str()
    );
}

/// Every mutating entry operation follows a symlink before its parent operand.
#[cfg(unix)]
#[test]
fn test_host_mutations_use_native_parent_target() {
    use std::io::Write;
    use std::os::unix::fs::symlink;

    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWriteOptions;
    for operation in [
        "copy",
        "rename",
        "delete",
        "writer",
        "directory",
        "temp-file",
        "temp-directory",
    ] {
        let dir = tempfile::tempdir().expect("isolated operation fixture");
        fs::create_dir(dir.path().join("a")).expect("a directory");
        fs::create_dir_all(dir.path().join("b/inner")).expect("b directory");
        fs::write(dir.path().join("a/config"), b"A").expect("a config");
        fs::write(dir.path().join("b/config"), b"B").expect("b config");
        symlink("../b/inner", dir.path().join("a/link")).expect("symlink fixture");
        let parent = dir.path().join("a/link/..");
        let config = parent.join("config");
        let output = parent.join("output");
        let host = LocalFileSystem::host().expect("host filesystem");
        match operation {
            "copy" => {
                let _ = host.copy(&config, &output).expect("copy native object");
                assert_eq!(fs::read(dir.path().join("b/output")).expect("copy result"), b"B");
            }
            "rename" => {
                let _ = host.rename(&config, &output).expect("rename native object");
                assert_eq!(fs::read(dir.path().join("b/output")).expect("rename result"), b"B");
                assert!(!dir.path().join("b/config").exists());
            }
            "delete" => {
                let _ = host.delete_file(&config).expect("delete native object");
                assert!(!dir.path().join("b/config").exists());
            }
            "writer" => {
                let mut writer = host
                    .open_writer_with_options(&config, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
                    .expect("open native object");
                writer.write_all(b"changed").expect("staged write");
                let _ = writer.commit().expect("publish native object");
                assert_eq!(
                    fs::read(dir.path().join("b/config")).expect("written result"),
                    b"changed"
                );
            }
            "directory" => {
                let _ = host.create_directory(&output).expect("create native directory");
                assert!(dir.path().join("b/output").is_dir());
                let _ = host.delete_directory(&output).expect("delete native directory");
                assert!(!dir.path().join("b/output").exists());
            }
            "temp-file" => {
                let temporary = host
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent))
                    .expect("create native temporary file");
                assert_eq!(fs::read_dir(dir.path().join("b")).expect("b entries").count(), 3);
                drop(temporary);
                assert_eq!(
                    fs::read_dir(dir.path().join("b")).expect("cleaned b entries").count(),
                    2
                );
            }
            "temp-directory" => {
                let temporary = host
                    .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&parent))
                    .expect("create native temporary directory");
                assert_eq!(fs::read_dir(dir.path().join("b")).expect("b entries").count(), 3);
                drop(temporary);
                assert_eq!(
                    fs::read_dir(dir.path().join("b")).expect("cleaned b entries").count(),
                    2
                );
            }
            _ => unreachable!("enumerated operation"),
        }
        assert_eq!(
            fs::read(dir.path().join("a/config")).expect("lexical neighbor intact"),
            b"A"
        );
        assert!(!dir.path().join("a/output").exists());
    }
}

/// Windows drive binding preserves raw dot components and rejects
/// drive-relative operands.
#[cfg(windows)]
#[test]
fn test_host_windows_drive_binding() {
    use qubit_local_files::path::LocalFileSystemScope;
    use qubit_local_files::path::LocalPathResolver;
    let resolver = LocalPathResolver::new(LocalFileSystemScope::Host, Path::new(r"C:\base")).expect("drive resolver");
    assert_eq!(
        resolver
            .resolve(Path::new(r"\dir\..\file"))
            .expect("root relative")
            .authority_relative()
            .as_os_str(),
        Path::new(r"C:\dir\..\file").as_os_str()
    );
    assert!(resolver.resolve(Path::new("C:file")).is_err());
    let verbatim =
        LocalPathResolver::new(LocalFileSystemScope::Host, Path::new(r"\\?\C:\base")).expect("verbatim resolver");
    assert_eq!(
        verbatim
            .resolve(Path::new(r"dir\..\file"))
            .expect("verbatim relative")
            .authority_relative()
            .as_os_str(),
        Path::new(r"\\?\C:\base\dir\..\file").as_os_str()
    );
}

/// Windows link/parent reads use the native oracle for ordinary and verbatim
/// paths.
#[cfg(windows)]
#[test]
fn test_host_windows_link_parent_reads_match_native() {
    use std::os::windows::fs::symlink_dir;
    let fixture = tempfile::tempdir().expect("Windows path fixture");
    fs::create_dir(fixture.path().join("a")).expect("lexical parent");
    fs::create_dir_all(fixture.path().join("b/inner")).expect("link destination");
    fs::write(fixture.path().join("a/config"), b"A").expect("lexical content");
    fs::write(fixture.path().join("b/config"), b"B").expect("native content");
    symlink_dir(fixture.path().join("b/inner"), fixture.path().join("a/link"))
        .expect("Windows runtime contract requires directory symlink privilege");
    let host = LocalFileSystem::host().expect("Host filesystem");
    for base in [
        fixture.path().to_path_buf(),
        fs::canonicalize(fixture.path()).expect("verbatim base"),
    ] {
        let mut operand = base.into_os_string();
        operand.push(r"\a\link\..\config");
        let path = Path::new(&operand);
        match fs::read(path) {
            Ok(expected) => assert_eq!(host.read_prefix(path, 8).expect("native read"), expected),
            Err(_) => assert!(host.read_prefix(path, 8).is_err(), "native-invalid operand must fail"),
        }
    }
}
