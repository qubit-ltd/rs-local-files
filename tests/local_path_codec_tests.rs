// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;

#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;

#[cfg(any(unix, windows))]
use proptest::{
    prelude::{
        Strategy,
        any,
        prop,
    },
    proptest,
};
use qubit_local_files::{
    LocalPathCodec,
    LocalPathCodecError,
};

/// Verifies native Unicode is retained while percent signs and controls use
/// canonical uppercase escaped bytes.
#[test]
fn test_decode_preserves_unicode_and_escapes_percent_and_control_bytes() {
    assert_eq!(
        LocalPathCodec::to_canonical_text(OsStr::new("文档%name\n"))
            .expect("native component should decode"),
        "文档%25name%0A",
    );
}

// Verifies every non-NUL Unix byte sequence round-trips through canonical
// text without losing native representation.
#[cfg(unix)]
proptest! {
    #[test]
    fn test_unix_non_nul_bytes_round_trip_through_canonical_text(
        bytes in prop::collection::vec(
            any::<u8>().prop_filter("native NUL is not representable", |byte| *byte != 0),
            0..256,
        ),
    ) {
        use std::{
            ffi::OsString,
            os::unix::ffi::{
                OsStringExt,
            },
        };

        let native = OsString::from_vec(bytes.clone());
        let canonical = LocalPathCodec::to_canonical_text(&native)
            .expect("non-NUL native bytes should decode");
        let encoded = LocalPathCodec::from_canonical_text(&canonical)
            .expect("canonical text should encode");
        assert_eq!(encoded.as_bytes(), bytes);
    }
}

// Verifies every non-NUL Windows UTF-16 sequence round-trips through
// canonical text without losing unpaired surrogates.
#[cfg(windows)]
proptest! {
    #[test]
    fn test_windows_non_nul_units_round_trip_through_canonical_text(
        units in prop::collection::vec(
            any::<u16>().prop_filter("native NUL is not representable", |unit| *unit != 0),
            0..256,
        ),
    ) {
        use std::{
            ffi::OsString,
            os::windows::ffi::{
                OsStrExt,
                OsStringExt,
            },
        };

        let native = OsString::from_wide(&units);
        let canonical = LocalPathCodec::to_canonical_text(&native)
            .expect("non-NUL native code units should decode");
        let encoded = LocalPathCodec::from_canonical_text(&canonical)
            .expect("canonical text should encode");
        assert_eq!(encoded.encode_wide().collect::<Vec<_>>(), units);
    }
}

/// Verifies aliases for literal text and lowercase escape digits are rejected.
#[test]
fn test_encode_rejects_non_canonical_escape_aliases() {
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%2fb"),
        Err(LocalPathCodecError::NonCanonicalText),
    ));
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%2Fb"),
        Err(LocalPathCodecError::NonCanonicalText),
    ));
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%41b"),
        Err(LocalPathCodecError::NonCanonicalText),
    ));
}

/// Verifies incomplete escape sequences identify the percent byte offset.
#[test]
fn test_encode_rejects_malformed_escape() {
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%4"),
        Err(LocalPathCodecError::InvalidEscape { offset: 1 }),
    ));
}

/// Verifies invalid hexadecimal escapes and native NUL bytes retain their
/// distinct canonical-path diagnostics.
#[test]
fn test_path_codec_rejects_invalid_hex_and_native_nul() {
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%G0"),
        Err(LocalPathCodecError::InvalidEscape { offset: 1 }),
    ));
    assert!(matches!(
        LocalPathCodec::from_canonical_text("a%00"),
        Err(LocalPathCodecError::NativeNul),
    ));
    assert!(matches!(
        LocalPathCodec::to_canonical_text(OsStr::new("a\0")),
        Err(LocalPathCodecError::NativeNul),
    ));
}

/// Verifies Unix non-UTF-8 native bytes round-trip through canonical text.
#[cfg(unix)]
#[test]
fn test_unix_non_utf8_native_bytes_round_trip() {
    use std::{
        ffi::OsString,
        os::unix::ffi::{
            OsStrExt,
            OsStringExt,
        },
    };

    let native = OsString::from_vec(vec![0x66, 0x80, 0x25]);
    let decoded = LocalPathCodec::to_canonical_text(&native)
        .expect("non-UTF-8 native component should decode");
    assert_eq!(decoded, "f%80%25");
    let encoded = LocalPathCodec::from_canonical_text(&decoded)
        .expect("canonical native component should encode");
    assert_eq!(encoded.as_bytes(), [0x66, 0x80, 0x25]);
}

/// Verifies Windows unpaired surrogate code units round-trip through canonical
/// text.
#[cfg(windows)]
#[test]
fn test_windows_unpaired_surrogate_round_trip() {
    use std::{
        ffi::OsString,
        os::windows::ffi::{
            OsStrExt,
            OsStringExt,
        },
    };

    let native = OsString::from_wide(&[0x0066, 0xD800, 0x0025]);
    let decoded = LocalPathCodec::to_canonical_text(&native)
        .expect("unpaired surrogate native component should decode");
    let encoded = LocalPathCodec::from_canonical_text(&decoded)
        .expect("canonical native component should encode");
    assert_eq!(
        encoded.encode_wide().collect::<Vec<_>>(),
        [0x0066, 0xD800, 0x0025]
    );
}
