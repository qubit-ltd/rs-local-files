// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;

use crate::LocalPathCodecError;
use crate::error::LocalFileErrorSource;
use crate::path::LocalFileNames;
use crate::path::LocalPathCodec;
use crate::path::LocalPaths;

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
