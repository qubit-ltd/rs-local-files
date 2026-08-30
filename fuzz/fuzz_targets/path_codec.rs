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

    #[cfg(unix)]
    if !data.contains(&0) {
        use std::os::unix::ffi::OsStringExt;

        let native = std::ffi::OsString::from_vec(data.to_vec());
        let canonical = LocalPathCodec::encode_component(&native).expect("non-NUL native bytes must convert");
        let restored = LocalPathCodec::decode_component(&canonical).expect("canonical native text must convert");
        assert_eq!(restored.as_encoded_bytes(), data);
        let canonical_again =
            LocalPathCodec::encode_component(&restored).expect("decoded native bytes must encode again");
        assert_eq!(canonical_again, canonical);
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::ffi::OsStringExt;

        let units = data
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        if units.iter().any(|unit| *unit == 0) {
            return;
        }
        let native = std::ffi::OsString::from_wide(&units);
        let canonical = LocalPathCodec::encode_component(&native).expect("non-NUL native UTF-16 must convert");
        let restored = LocalPathCodec::decode_component(&canonical).expect("canonical native text must convert");
        assert_eq!(restored.encode_wide().collect::<Vec<_>>(), units);
        let canonical_again =
            LocalPathCodec::encode_component(&restored).expect("decoded native UTF-16 must encode again");
        assert_eq!(canonical_again, canonical);
    }
});
