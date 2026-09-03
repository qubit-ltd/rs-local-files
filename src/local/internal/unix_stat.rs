// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Portable interpretation of native Unix stat mode fields.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ops::BitAnd;

/// Tests whether a native stat mode represents a regular file.
///
/// Android's 32-bit stat structure widens `st_mode` beyond `mode_t`; the
/// generic conversion preserves that platform ABI without target-specific
/// casts at every call site.
#[must_use]
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline(always))]
#[cfg_attr(coverage, inline(never))]
pub(crate) fn is_regular_file_mode<T>(mode: T) -> bool
where
    T: BitAnd<Output = T> + Copy + From<libc::mode_t> + PartialEq,
{
    let file_type_mask = T::from(libc::S_IFMT);
    let regular_file_type = T::from(libc::S_IFREG);
    mode & file_type_mask == regular_file_type
}
