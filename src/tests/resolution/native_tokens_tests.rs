// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::directory::LocalResolutionOrigin;
use crate::directory::LocalResolutionScope;
use crate::options::LocalResolutionLimits;
use crate::resolution::ResolutionContract;
use crate::resolution::native_tokens::NativeToken;
use crate::resolution::native_tokens::measure_path;
use crate::resolution::native_tokens::tokenize;
use crate::resolution::session::ResolutionSession;

/// Builds a test session with explicit pending component and byte limits.
fn session(max_components: usize, max_bytes: usize) -> ResolutionSession {
    let contract = ResolutionContract::unrestricted().tightened(
        LocalResolutionLimits::new()
            .with_max_actual_components(max_components)
            .with_max_resolved_path_bytes(max_bytes),
    );
    ResolutionSession::new(&contract, LocalResolutionScope::Descendant)
}

#[test]
fn test_tokenizer_preserves_dot_parent_components_and_origin() {
    let tokens = tokenize(
        Path::new("a/./../b"),
        LocalResolutionOrigin::SymlinkTarget,
        &session(4, 64),
    )
    .unwrap();
    assert_eq!(
        tokens,
        vec![
            NativeToken::Normal("a".into(), LocalResolutionOrigin::SymlinkTarget),
            NativeToken::Current(LocalResolutionOrigin::SymlinkTarget),
            NativeToken::Parent(LocalResolutionOrigin::SymlinkTarget),
            NativeToken::Normal("b".into(), LocalResolutionOrigin::SymlinkTarget),
        ],
        "dot and parent components must retain order and origin"
    );
}

#[cfg(any(unix, windows))]
#[test]
fn test_tokenizer_budget_matches_native_path_units() {
    let path = Path::new("a/./../b");
    #[cfg(unix)]
    let expected_bytes = 9;
    #[cfg(windows)]
    let expected_bytes = 18;
    assert_eq!(
        Some((4, expected_bytes)),
        measure_path(path),
        "path measurement must use native encoding units"
    );
    assert!(
        tokenize(path, LocalResolutionOrigin::SymlinkTarget, &session(4, expected_bytes)).is_ok(),
        "the exact native-unit budget must accept the path"
    );
    let error = tokenize(
        path,
        LocalResolutionOrigin::SymlinkTarget,
        &session(4, expected_bytes - 1),
    )
    .unwrap_err();
    assert_eq!(
        crate::LocalFileErrorKind::ResourceLimit,
        error.kind(),
        "a budget below the native-unit measurement must be rejected"
    );
}

#[cfg(unix)]
#[test]
fn test_tokenizer_preserves_non_utf8_component_names() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let value = std::ffi::OsString::from_vec(vec![b'x', b'/', 0xff]);
    let tokens = tokenize(Path::new(&value), LocalResolutionOrigin::Input, &session(2, 4)).unwrap();
    assert!(
        matches!(&tokens[1], NativeToken::Normal(name, _) if name.as_bytes() == [0xff]),
        "tokenization must preserve a non-UTF-8 component byte"
    );
}

#[cfg(unix)]
#[test]
fn test_tokenizer_checks_exact_limit_before_building_tokens() {
    assert_eq!(
        measure_path(Path::new("a/b")),
        Some((2, 4)),
        "each component and separator must contribute to the pending budget"
    );
    assert_eq!(
        tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(2, 4))
            .unwrap()
            .len(),
        2,
        "the exact component and byte limits must be accepted"
    );
    let error = tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(2, 3)).unwrap_err();
    assert_eq!(
        error.resource_limit_error().unwrap().resource(),
        crate::error::LocalResourceKind::PendingResolutionBytes,
        "the byte limit must fail before token construction"
    );
    let error = tokenize(Path::new("a/b"), LocalResolutionOrigin::Input, &session(1, 4)).unwrap_err();
    assert_eq!(
        error.resource_limit_error().unwrap().resource(),
        crate::error::LocalResourceKind::PendingResolutionComponents,
        "the component limit must fail before token construction"
    );
}

#[cfg(unix)]
#[test]
fn test_tokenizer_preserves_trailing_directory_marker_without_budget_charge() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let origin = LocalResolutionOrigin::Input;
    let path = Path::new("///a//");
    assert_eq!(
        measure_path(path),
        Some((1, 2)),
        "repeated separators do not add budget charges"
    );
    assert_eq!(
        tokenize(path, origin, &session(1, 2)).unwrap(),
        vec![NativeToken::Normal("a".into(), origin), NativeToken::RequireDirectory,],
        "one trailing directory marker must survive without a budget charge"
    );
    assert_eq!(
        tokenize(Path::new("/"), origin, &session(0, 0)).unwrap(),
        vec![NativeToken::RequireDirectory],
        "a root-only path must still require a directory"
    );
    assert_eq!(
        tokenize(Path::new("a/."), origin, &session(2, 4)).unwrap(),
        vec![NativeToken::Normal("a".into(), origin), NativeToken::Current(origin)],
        "a final dot component is not a trailing separator marker"
    );
    let non_utf8 = std::ffi::OsString::from_vec(vec![b'x', 0xff, b'/']);
    assert!(
        matches!(
            tokenize(Path::new(&non_utf8), origin, &session(1, 3)).unwrap().as_slice(),
            [NativeToken::Normal(name, _), NativeToken::RequireDirectory]
                if name.as_bytes() == [b'x', 0xff]
        ),
        "a non-UTF-8 name and trailing directory marker must both be retained"
    );
}

#[cfg(windows)]
#[test]
fn test_tokenizer_preserves_windows_trailing_directory_separators() {
    for path in [r"C:\a\", "C:/a/"] {
        assert!(
            matches!(
                tokenize(Path::new(path), LocalResolutionOrigin::Input, &session(1, 4))
                    .unwrap()
                    .last(),
                Some(NativeToken::RequireDirectory)
            ),
            "a Windows trailing separator must emit a directory marker"
        );
    }
}

#[cfg(windows)]
#[test]
fn test_windows_unc_tokenization_emits_only_descendants() {
    for value in [r"\\server\share\foo", r"\\?\UNC\server\share\foo"] {
        let path = Path::new(value);
        assert_eq!(
            measure_path(path),
            Some((1, 8)),
            "UNC server and share anchors must not count as descendants"
        );
        assert_eq!(
            tokenize(path, LocalResolutionOrigin::Input, &session(1, 8)).unwrap(),
            vec![NativeToken::Normal("foo".into(), LocalResolutionOrigin::Input)],
            "UNC tokenization must emit only the descendant component"
        );
    }
}

#[cfg(windows)]
#[test]
fn test_non_bmp_pending_bytes_use_utf16_units() {
    assert_eq!(
        measure_path(Path::new("a/😀")),
        Some((2, 10)),
        "non-BMP names must count UTF-16 code units for pending bytes"
    );
}
