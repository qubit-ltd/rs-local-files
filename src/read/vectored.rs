// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fallback implementation for platforms without native vectored reads.

use std::io;
use std::io::IoSliceMut;
use std::io::Read;

/// Reads sequentially into vectored buffers while preserving `Read` progress
/// semantics.
///
/// Empty buffers are skipped. A short read or EOF stops the operation, while an
/// error after any successful read returns the accumulated byte count. An error
/// on the first read is returned unchanged.
#[cfg(any(windows, test))]
pub(crate) fn read_vectored_fallback<R: Read + ?Sized>(
    reader: &mut R,
    buffers: &mut [IoSliceMut<'_>],
) -> io::Result<usize> {
    let mut total = 0_usize;
    for buffer in buffers {
        if buffer.is_empty() {
            continue;
        }
        let count = match reader.read(buffer) {
            Ok(count) => count,
            Err(_) if total > 0 => return Ok(total),
            Err(error) => return Err(error),
        };
        total += count;
        if count < buffer.len() {
            break;
        }
    }
    Ok(total)
}
