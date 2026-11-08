// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Seek;
use std::io::SeekFrom;

use digest::Digest;
use digest::Output;

use crate::local::internal::conditional::ConditionalEntry;

/// Computes a digest of the complete contents of a retained conditional entry.
///
/// The entry is reopened through its retained access authority, and reading
/// starts at byte zero regardless of the reader's initial position. This helper
/// does not modify the entry.
///
/// # Type Parameters
///
/// * `D` is the digest algorithm used to process the entry contents.
///
/// # Parameters
///
/// * `entry` is the retained entry to reopen and read.
///
/// # Errors
///
/// Returns an I/O error if reopening the entry, seeking to its beginning, or
/// reading its contents fails.
pub(super) fn digest_entry<D: Digest>(entry: &ConditionalEntry) -> std::io::Result<Output<D>> {
    let mut reader = entry.open_reader().map_err(std::io::Error::other)?;
    reader.seek(SeekFrom::Start(0))?;
    qubit_io::DigestInput::<_, D>::digest_all(&mut reader)
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
