// =============================================================================

use std::io::Seek;
use std::io::SeekFrom;

use digest::Digest;
use digest::Output;

use crate::local::internal::conditional::ConditionalEntry;

pub(super) fn digest_entry<D: Digest>(entry: &ConditionalEntry) -> std::io::Result<Output<D>> {
    let mut reader = entry
        .open_reader()
        .map_err(|error| std::io::Error::other(error))?;
    reader.seek(SeekFrom::Start(0))?;
    qubit_io::DigestInput::<_, D>::digest_all(&mut reader)
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
