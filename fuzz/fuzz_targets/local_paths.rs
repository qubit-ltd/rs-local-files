// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exercises canonical relative-path composition and decomposition.

#![no_main]

use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalPaths;

const MAX_FUZZ_INPUT_LEN: usize = 4096;
const MAX_COMPONENTS: usize = 128;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let text = String::from_utf8_lossy(data);
    let components = text.split('\0').take(MAX_COMPONENTS).collect::<Vec<_>>();

    let Ok(native) = LocalPaths::from_canonical_relative_components(
        components.iter().copied(),
    ) else {
        return;
    };
    let encoded = LocalPaths::to_canonical_relative_components(Path::new(&native))
        .expect("validated relative paths must encode");
    let restored = LocalPaths::from_canonical_relative_components(
        encoded.iter().map(String::as_str),
    )
    .expect("encoded relative paths must decode");
    assert_eq!(restored, native);
});
