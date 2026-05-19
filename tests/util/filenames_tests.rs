/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::path::Path;

use qubit_local_fs::Filenames;

#[test]
fn test_random_and_try_random_use_default_prefix() {
    let infallible_name = Filenames::random();
    let fallible_name = Filenames::try_random().expect("random name should be generated");

    assert!(infallible_name.starts_with(Filenames::DEFAULT_RANDOM_PREFIX));
    assert!(fallible_name.starts_with(Filenames::DEFAULT_RANDOM_PREFIX));
    assert_ne!(infallible_name, fallible_name);
}

#[test]
fn test_validate_portable_file_name_accepts_safe_names() {
    for name in [
        "report.txt",
        "archive.tar.gz",
        ".env",
        "my file.txt",
        "data_2026-05-19.csv",
        "caf\u{00e9}.txt",
    ] {
        Filenames::validate_portable_file_name(name)
            .expect("safe portable file name should be accepted");
    }
}

#[test]
fn test_validate_portable_file_name_rejects_empty_dot_and_dot_dot() {
    for name in ["", ".", ".."] {
        let error = Filenames::validate_portable_file_name(name)
            .expect_err("invalid dot segment should be rejected");

        assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
    }
}

#[test]
fn test_validate_portable_file_name_rejects_path_and_reserved_characters() {
    for name in [
        "dir/file.txt",
        r"dir\file.txt",
        "name\0.txt",
        "bad:name.txt",
        "bad<name>.txt",
        "bad|name.txt",
        "bad?name.txt",
        "bad*name.txt",
        "bad\"name.txt",
        "line\nbreak.txt",
    ] {
        let error = Filenames::validate_portable_file_name(name)
            .expect_err("forbidden character should be rejected");

        assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
    }
}

#[test]
fn test_validate_portable_file_name_rejects_windows_reserved_names() {
    for name in [
        "CON", "con", "CON.txt", "PRN", "AUX", "NUL", "COM1", "com9.log", "LPT1", "lpt9.txt",
        "CONIN$", "CONOUT$",
    ] {
        let error = Filenames::validate_portable_file_name(name)
            .expect_err("Windows reserved device name should be rejected");

        assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
    }

    Filenames::validate_portable_file_name("COM0.txt").expect("COM0 should not be reserved");
    Filenames::validate_portable_file_name("COM10.txt").expect("COM10 should not be reserved");
    Filenames::validate_portable_file_name("LPT0.txt").expect("LPT0 should not be reserved");
}

#[test]
fn test_validate_portable_file_name_rejects_trailing_space_dot_and_long_names() {
    for name in ["file.", "file "] {
        let error = Filenames::validate_portable_file_name(name)
            .expect_err("trailing space or dot should be rejected");

        assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
    }

    let max_name = "a".repeat(255);
    let too_long_name = "a".repeat(256);

    Filenames::validate_portable_file_name(&max_name).expect("255-byte name should be accepted");
    let error = Filenames::validate_portable_file_name(&too_long_name)
        .expect_err("name longer than 255 bytes should be rejected");

    assert_eq!(std::io::ErrorKind::InvalidInput, error.kind());
}

#[test]
fn test_file_name_returns_final_component() {
    let path = Path::new("/tmp/archive.tar.gz");

    assert_eq!(Some("archive.tar.gz"), Filenames::file_name(path));
    assert_eq!(None, Filenames::file_name(Path::new("/")));
}

#[test]
fn test_file_stem_prefix_and_extension_follow_path_semantics() {
    let path = Path::new("/tmp/archive.tar.gz");

    assert_eq!(Some("archive.tar"), Filenames::file_stem(path));
    assert_eq!(Some("archive"), Filenames::file_prefix(path));
    assert_eq!(Some("gz"), Filenames::extension(path));
    assert_eq!(Some(".gz".to_owned()), Filenames::dot_extension(path));
}

#[test]
fn test_extension_helpers_handle_missing_and_empty_extensions() {
    assert_eq!(None, Filenames::extension(Path::new("README")));
    assert_eq!(None, Filenames::dot_extension(Path::new("README")));
    assert_eq!(Some(""), Filenames::extension(Path::new("name.")));
    assert_eq!(
        Some(String::new()),
        Filenames::dot_extension(Path::new("name."))
    );
}

#[test]
fn test_dotfiles_follow_rust_path_semantics() {
    assert_eq!(Some(".env"), Filenames::file_stem(Path::new(".env")));
    assert_eq!(None, Filenames::extension(Path::new(".env")));

    assert_eq!(
        Some(".config"),
        Filenames::file_stem(Path::new(".config.toml"))
    );
    assert_eq!(
        Some("toml"),
        Filenames::extension(Path::new(".config.toml"))
    );
}

#[test]
fn test_has_extension_accepts_optional_leading_dot() {
    let path = Path::new("report.PDF");

    assert!(Filenames::has_extension(path, "PDF"));
    assert!(Filenames::has_extension(path, ".PDF"));
    assert!(!Filenames::has_extension(path, "pdf"));
    assert!(Filenames::has_extension_ignore_ascii_case(path, "pdf"));
    assert!(Filenames::has_extension_ignore_ascii_case(path, ".pdf"));
}

#[test]
fn test_file_name_from_path_handles_common_separators() {
    assert_eq!(
        "file.txt",
        Filenames::file_name_from_path("/tmp/data/file.txt")
    );
    assert_eq!(
        "file.txt",
        Filenames::file_name_from_path(r"C:\tmp\data\file.txt")
    );
    assert_eq!("file.txt", Filenames::file_name_from_path("file.txt"));
    assert_eq!("", Filenames::file_name_from_path("/tmp/data/"));
}

#[test]
fn test_file_name_from_url_removes_query_and_fragment() {
    assert_eq!(
        "file.txt",
        Filenames::file_name_from_url("https://example.com/path/file.txt?download=1")
    );
    assert_eq!(
        "file.txt",
        Filenames::file_name_from_url("https://example.com/path/file.txt#section")
    );
    assert_eq!(
        "file.txt",
        Filenames::file_name_from_url("https://example.com/path/file.txt?download=1#section")
    );
}

#[test]
fn test_file_name_from_url_decodes_percent_encoded_utf8() {
    assert_eq!(
        "my file.txt",
        Filenames::file_name_from_url("https://example.com/path/my%20file.txt")
    );
    assert_eq!(
        format!("caf{}.txt", '\u{00e9}'),
        Filenames::file_name_from_url("https://example.com/path/caf%C3%A9.txt")
    );
    assert_eq!(
        "file+plus.txt",
        Filenames::file_name_from_url("https://example.com/path/file%2Bplus.txt")
    );
}

#[test]
fn test_file_name_from_url_keeps_invalid_percent_encoding() {
    assert_eq!(
        "file%ZZ.txt",
        Filenames::file_name_from_url("https://example.com/path/file%ZZ.txt")
    );
    assert_eq!(
        "file%2.txt",
        Filenames::file_name_from_url("https://example.com/path/file%2.txt")
    );
}
