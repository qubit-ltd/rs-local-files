// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;

use qubit_local_files::path::PortableFileName;

/// Verifies portable file names retain validated UTF-8 text.
#[test]
fn test_try_from_accepts_portable_name() {
    let name = PortableFileName::try_from("résumé.txt")
        .expect("the portable Unicode filename should be accepted");

    assert_eq!("résumé.txt", name.as_str());
    assert_eq!("résumé.txt", name.as_ref());
    assert_eq!("résumé.txt", name.to_string());
}

/// Verifies owned text uses the same validation and representation.
#[test]
fn test_try_from_owned_string_accepts_portable_name() {
    let name = PortableFileName::try_from(String::from("payload.bin"))
        .expect("the owned portable filename should be accepted");

    assert_eq!("payload.bin", name.as_str());
}

/// Verifies reserved and path-bearing inputs cannot become portable names.
#[test]
fn test_try_from_rejects_reserved_or_composite_name() {
    for invalid in ["", ".", "..", "CON", "a/b", "a\\b", "trailing."] {
        let error = PortableFileName::try_from(invalid)
            .expect_err("the invalid portable filename should be rejected");
        assert_eq!(ErrorKind::InvalidInput, error.kind(), "input: {invalid}");
    }
}
