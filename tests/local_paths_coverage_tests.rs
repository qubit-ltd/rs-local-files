// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalPaths;

/// Verifies canonical conversions reject empty, separator-bearing, and invalid
/// root shapes before composing native paths.
#[test]
fn test_scope_aware_path_conversions_reject_unsafe_component_shapes() {
    let rooted = LocalPaths::rooted();
    for components in [vec![""], vec!["safe", ""], vec!["safe", "%2F"]] {
        let error = rooted
            .from_canonical_components(components)
            .expect_err("relative canonical components must be normal paths");
        assert_eq!(LocalFileOperation::ComposePath, error.operation());
    }

    let error = rooted
        .to_canonical_components(Path::new("/absolute"))
        .expect_err("absolute paths cannot be encoded as relative components");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());

    #[cfg(unix)]
    {
        let host = LocalPaths::host();
        let root = host
            .from_canonical_components(Vec::<&str>::new())
            .expect("the Unix root is a valid canonical absolute path");
        assert_eq!(Path::new("/"), root);

        let error = host
            .from_canonical_components(["%2F"])
            .expect_err("an encoded separator cannot be an absolute component");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    }

    let codec_error = rooted
        .from_canonical_components(["%2f"])
        .expect_err("lowercase escape should produce a path codec error");
    assert_eq!(LocalFileErrorKind::InvalidPath, codec_error.kind());
}

/// Verifies direct bindings and conversions cover both successful relative
/// paths and their empty or dot-component rejection cases.
#[test]
fn test_path_conversions_cover_rooted_binding_and_normal_components() {
    let rooted = LocalPaths::rooted();
    let canonical = rooted
        .from_canonical_components(["safe", "nested"])
        .expect("vector-backed canonical components should decode");
    assert_eq!(Path::new("safe/nested"), canonical);
    assert_eq!(
        vec!["safe".to_owned(), "nested".to_owned()],
        rooted
            .to_canonical_components(&canonical)
            .expect("normal relative components should encode"),
    );
    assert!(rooted.from_canonical_components(Vec::<&str>::new()).is_ok());
    assert!(rooted.to_canonical_components(Path::new(".")).is_err());
    assert_eq!(
        Path::new("single"),
        rooted
            .from_canonical_components(["single"])
            .expect("iterator-backed relative components should decode"),
    );

    #[cfg(unix)]
    {
        let host = LocalPaths::host();
        let absolute = host
            .from_canonical_components(["tmp", "coverage"])
            .expect("vector-backed absolute components should decode");
        assert_eq!(Path::new("/tmp/coverage"), absolute);
        assert_eq!(
            vec!["tmp".to_owned(), "coverage".to_owned()],
            host.to_canonical_components(&absolute)
                .expect("normal absolute components should encode"),
        );
        assert!(
            host.to_canonical_components(Path::new("/tmp/./coverage"))
                .is_err()
        );
        assert_eq!(
            Path::new("/"),
            host.from_canonical_components(Vec::<&str>::new())
                .expect("iterator-backed root components should decode"),
        );
    }
}

/// Verifies non-UTF-8 Unix native components are represented losslessly by
/// canonical escaped-byte text.
#[cfg(unix)]
#[test]
fn test_host_conversions_encode_non_utf8_native_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let relative = std::path::PathBuf::from(OsString::from_vec(vec![0xff]));
    assert_eq!(
        vec!["%FF".to_owned()],
        LocalPaths::rooted()
            .to_canonical_components(&relative)
            .expect("non-UTF-8 relative component should encode losslessly"),
    );

    let mut absolute = std::path::PathBuf::from("/tmp");
    absolute.push(OsString::from_vec(vec![0xff]));
    assert_eq!(
        vec!["tmp".to_owned(), "%FF".to_owned()],
        LocalPaths::host()
            .to_canonical_components(&absolute)
            .expect("non-UTF-8 absolute component should encode losslessly"),
    );
}
