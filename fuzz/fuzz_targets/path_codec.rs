// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exercises native-path and URI-component codec boundaries with arbitrary
//! input.
//!
//! Inputs are bounded to keep parser allocations and fuzz iterations useful.

#![no_main]

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalPathCodec;

/// Bounds direct fuzzer input and the codec's temporary allocations.
const MAX_FUZZ_INPUT_LEN: usize = 4096;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];

    if let Ok(text) = std::str::from_utf8(data)
        && let Ok(canonical) = LocalPathCodec::decode_uri_component(text)
    {
        assert_eq!(
            canonical,
            LocalPathCodec::decode_uri_component(&canonical)
                .expect("canonical URI component must reparse"),
        );
    }

    #[cfg(unix)]
    if !data.contains(&0) {
        use std::os::unix::ffi::OsStringExt;

        let native = std::ffi::OsString::from_vec(data.to_vec());
        let canonical = LocalPathCodec::decode(&native)
            .expect("non-NUL native bytes must decode");
        let restored = LocalPathCodec::encode(&canonical)
            .expect("canonical native text must encode");
        assert_eq!(restored.as_encoded_bytes(), data);
    }
});
