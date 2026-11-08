// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;

#[cfg(any(unix, windows))]
use proptest::prelude::Strategy;
#[cfg(any(unix, windows))]
use proptest::prelude::any;
#[cfg(any(unix, windows))]
use proptest::prelude::prop;
#[cfg(any(unix, windows))]
use proptest::proptest;
use qubit_local_files::LocalFileError;
use qubit_local_files::error::LocalFileErrorSource;
use qubit_local_files::error::LocalPathCodecError;
use qubit_local_files::path::LocalFileNames;
use qubit_local_files::path::LocalPathCodec;
use qubit_local_files::path::LocalPaths;

/// Verifies Unicode is retained while percent and controls use canonical
/// uppercase escapes.
#[test]
fn test_encode_component_preserves_unicode_and_escapes_special_bytes() {
    assert_eq!(
        "文档%25name%0A",
        LocalPathCodec::encode_component(OsStr::new("文档%name\n")).expect("native component should encode"),
    );
}

// Verifies every non-NUL Unix byte sequence round-trips without losing its
// native representation.
#[cfg(unix)]
proptest! {
    #[test]
    fn test_unix_non_nul_bytes_round_trip_through_canonical_text(
        bytes in prop::collection::vec(
            any::<u8>().prop_filter("native NUL is not representable", |byte| *byte != 0),
            0..256,
        ),
    ) {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::ffi::OsStringExt;

        let native = OsString::from_vec(bytes.clone());
        let canonical = LocalPathCodec::encode_component(&native)
            .expect("non-NUL native bytes should encode");
        let decoded = LocalPathCodec::decode_component(&canonical)
            .expect("canonical text should decode");
        assert_eq!(decoded.as_bytes(), bytes);
    }
}

// Verifies every non-NUL Windows UTF-16 sequence round-trips without losing
// unpaired surrogates.
#[cfg(windows)]
proptest! {
    #[test]
    fn test_windows_non_nul_units_round_trip_through_canonical_text(
        units in prop::collection::vec(
            any::<u16>().prop_filter("native NUL is not representable", |unit| *unit != 0),
            0..256,
        ),
    ) {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;

        let native = OsString::from_wide(&units);
        let canonical = LocalPathCodec::encode_component(&native)
            .expect("non-NUL native code units should encode");
        let decoded = LocalPathCodec::decode_component(&canonical)
            .expect("canonical text should decode");
        assert_eq!(decoded.encode_wide().collect::<Vec<_>>(), units);
    }
}

/// Verifies aliases for literal text and lowercase escapes are rejected.
#[test]
fn test_decode_component_rejects_non_canonical_escape_aliases() {
    for component in ["a%2fb", "a%2Fb", "a%41b"] {
        assert_codec_error(
            LocalPathCodec::decode_component(component).expect_err("escape alias must be rejected"),
            LocalPathCodecError::NonCanonicalText,
        );
    }
}

/// Verifies malformed escapes and native NUL retain their typed diagnostics.
#[test]
fn test_path_codec_rejects_malformed_escape_and_native_nul() {
    assert_codec_error(
        LocalPathCodec::decode_component("a%4").expect_err("incomplete escape must be rejected"),
        LocalPathCodecError::InvalidEscape { offset: 1 },
    );
    assert_codec_error(
        LocalPathCodec::decode_component("a%G0").expect_err("invalid hexadecimal escape must be rejected"),
        LocalPathCodecError::InvalidEscape { offset: 1 },
    );
    assert_codec_error(
        LocalPathCodec::decode_component("a%00").expect_err("native NUL must be rejected"),
        LocalPathCodecError::NativeNul,
    );
    assert_codec_error(
        LocalPathCodec::encode_component(OsStr::new("a\0")).expect_err("native NUL must be rejected"),
        LocalPathCodecError::NativeNul,
    );
}

/// Verifies Unix non-UTF-8 native bytes round-trip through canonical text.
#[cfg(unix)]
#[test]
fn test_unix_non_utf8_native_bytes_round_trip() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let native = OsString::from_vec(vec![0x66, 0x80, 0x25]);
    let canonical = LocalPathCodec::encode_component(&native).expect("non-UTF-8 native component should encode");
    assert_eq!(canonical, "f%80%25");
    let decoded = LocalPathCodec::decode_component(&canonical).expect("canonical native component should decode");
    assert_eq!(decoded.as_bytes(), [0x66, 0x80, 0x25]);
}

/// Verifies Windows unpaired surrogate code units round-trip through canonical
/// text.
#[cfg(windows)]
#[test]
fn test_windows_unpaired_surrogate_round_trip() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;

    let native = OsString::from_wide(&[0x0066, 0xD800, 0x0025]);
    let canonical =
        LocalPathCodec::encode_component(&native).expect("unpaired surrogate native component should encode");
    let decoded = LocalPathCodec::decode_component(&canonical).expect("canonical native component should decode");
    assert_eq!(decoded.encode_wide().collect::<Vec<_>>(), [0x0066, 0xD800, 0x0025]);
}

/// Asserts that a structured path error retains one expected codec failure.
///
/// # Parameters
///
/// - `error`: Structured public error returned by the codec.
/// - `expected`: Expected typed codec source.
///
/// # Panics
///
/// Panics when the typed source is absent or differs from `expected`.
fn assert_codec_error(error: LocalFileError, expected: LocalPathCodecError) {
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::PathCodec(actual)) if *actual == expected,
    ));
}

/// Exercises direct canonical decoding independently of encoder-generated
/// input.
#[test]
fn test_decode_component_direct_text_boundaries() {
    for text in ["%", "%0", "%GG"] {
        assert_codec_error(
            LocalPathCodec::decode_component(text).expect_err("malformed escape must fail"),
            LocalPathCodecError::InvalidEscape { offset: 0 },
        );
    }
    for text in ["%0a", "%41", "%2F"] {
        assert_codec_error(
            LocalPathCodec::decode_component(text).expect_err("noncanonical text must fail"),
            LocalPathCodecError::NonCanonicalText,
        );
    }
    for (text, native) in [("%0A", "\n"), ("%25", "%"), ("/", "/"), ("..", "..")] {
        let decoded = LocalPathCodec::decode_component(text).expect("canonical text must decode");
        assert_eq!(decoded, OsStr::new(native));
        assert_eq!(
            LocalPathCodec::encode_component(&decoded).expect("decoded text must encode"),
            text
        );
    }
}

#[test]
fn test_codec_preserves_canonical_native_data_and_rejects_aliases() {
    let encoded =
        LocalPathCodec::encode_component(OsStr::new("100%\nreport")).expect("representable native data should encode");
    assert_eq!("100%25%0Areport", encoded);
    assert_eq!(
        OsStr::new("100%\nreport"),
        LocalPathCodec::decode_component(&encoded).expect("canonical data should decode"),
    );

    for (text, expected) in [
        ("%", LocalPathCodecError::InvalidEscape { offset: 0 }),
        ("%G0", LocalPathCodecError::InvalidEscape { offset: 0 }),
        ("%2f", LocalPathCodecError::NonCanonicalText),
    ] {
        let error = LocalPathCodec::decode_component(text).expect_err("invalid spelling must fail");
        assert!(matches!(
            error.typed_source(),
            Some(LocalFileErrorSource::PathCodec(actual)) if *actual == expected,
        ));
    }
}

#[cfg(unix)]
#[test]
fn test_codec_round_trips_non_utf8_bytes_and_rejects_native_nul() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let native = OsString::from_vec(vec![b'a', 0x80, b'%']);
    let encoded = LocalPathCodec::encode_component(&native).expect("non-NUL bytes should encode");
    assert_eq!("a%80%25", encoded);
    let decoded = LocalPathCodec::decode_component(&encoded).expect("canonical bytes should decode");
    assert_eq!(native.as_bytes(), decoded.as_bytes());

    let nul = OsString::from_vec(vec![b'a', 0, b'b']);
    let error = LocalPathCodec::encode_component(&nul).expect_err("native NUL must fail");
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::PathCodec(LocalPathCodecError::NativeNul)),
    ));
}

#[test]
fn test_filename_validation_rejects_portable_and_bounded_violations() {
    let portable = LocalFileNames::portable();
    for invalid in ["", ".", "..", "CON.txt", "trailing.", "slash/name", "back\\slash"] {
        assert!(
            portable.validate(OsStr::new(invalid)).is_err(),
            "must reject {invalid:?}"
        );
    }
    assert!(portable.with_max_component_bytes(0).is_err());

    let bounded = portable
        .with_max_component_bytes(3)
        .expect("positive filename limit should be accepted");
    assert!(bounded.validate(OsStr::new("four")).is_err());
    bounded
        .validate(OsStr::new("two"))
        .expect("within-limit name should pass");
}

#[test]
fn test_scope_path_codec_round_trips_and_rejects_parent_escape() {
    let paths = LocalPaths::rooted();
    let native = paths
        .from_canonical_components(["safe", "escaped%25name"])
        .expect("canonical rooted path should decode");
    assert_eq!(
        vec!["safe", "escaped%25name"],
        paths
            .to_canonical_components(&native)
            .expect("rooted path should encode canonically"),
    );
    assert!(paths.from_canonical_components(["safe", ".."]).is_err());
}
