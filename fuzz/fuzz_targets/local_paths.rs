// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exercises scope-aware canonical path composition and decomposition.

#![no_main]

use std::path::{Path, PathBuf};

use libfuzzer_sys::fuzz_target;
use qubit_local_files::{LocalFileSystemScope, LocalPaths};

const MAX_FUZZ_INPUT_LEN: usize = 4096;
const MAX_COMPONENTS: usize = 128;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let text = String::from_utf8_lossy(data);
    let components = text.split('\0').take(MAX_COMPONENTS).collect::<Vec<_>>();

    let Ok(native) = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        components.iter().copied(),
    ) else {
        return;
    };
    let encoded =
        LocalPaths::to_canonical_components(LocalFileSystemScope::Rooted, Path::new(&native))
            .expect("validated rooted paths must encode");
    let restored = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        encoded.iter().map(String::as_str),
    )
    .expect("encoded rooted paths must decode");
    assert_eq!(restored, native);
    let reencoded =
        LocalPaths::to_canonical_components(LocalFileSystemScope::Rooted, Path::new(&restored))
            .expect("decoded rooted paths must encode again");
    assert_eq!(reencoded, encoded);

    #[cfg(unix)]
    fuzz_host_unix(data);

    #[cfg(windows)]
    fuzz_host_windows(data);
});

#[cfg(unix)]
fn fuzz_host_unix(data: &[u8]) {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let mut native = PathBuf::from("/");
    for component in data
        .split(|byte| *byte == 0 || *byte == b'/')
        .take(MAX_COMPONENTS)
    {
        if component.is_empty() || component == b"." || component == b".." {
            continue;
        }
        native.push(OsString::from_vec(component.to_vec()));
    }
    let Ok(encoded) = LocalPaths::to_canonical_components(LocalFileSystemScope::Host, &native)
    else {
        return;
    };
    let restored = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Host,
        encoded.iter().map(String::as_str),
    )
    .expect("encoded Unix host paths must decode");
    assert_eq!(restored, native);
}

#[cfg(windows)]
fn fuzz_host_windows(data: &[u8]) {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let mut native = PathBuf::from("C:\\");
    for units in data
        .chunks(2)
        .take(MAX_COMPONENTS)
        .map(|pair| u16::from_le_bytes([pair[0], *pair.get(1).unwrap_or(&0)]))
    {
        if units == 0 || units == b'/' as u16 || units == b'\\' as u16 {
            continue;
        }
        native.push(OsString::from_wide(&[units]));
    }
    let Ok(encoded) = LocalPaths::to_canonical_components(LocalFileSystemScope::Host, &native)
    else {
        return;
    };
    let restored = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Host,
        encoded.iter().map(String::as_str),
    )
    .expect("encoded Windows host paths must decode");
    assert_eq!(restored, native);
}
