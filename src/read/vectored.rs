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

use qubit_io::Input;

/// Reads sequentially into vectored buffers while preserving `Read` progress
/// semantics.
///
/// Empty buffers are skipped. A short read or EOF stops the operation, while an
/// error after any successful read returns the accumulated byte count. An error
/// on the first read is returned unchanged.
///
/// # Parameters
///
/// - `reader`: source of bytes consumed in buffer order.
/// - `buffers`: destination buffers filled sequentially; empty buffers are
///   ignored.
///
/// # Returns
///
/// The number of bytes read before a short read, EOF, or a later read error.
///
/// # Errors
///
/// Returns the first read error unchanged when no bytes were read. Errors after
/// a successful read are suppressed and the accumulated byte count is returned.
#[cfg(any(windows, test))]
pub(crate) fn read_vectored_fallback<R: Input<Item = u8> + ?Sized>(
    reader: &mut R,
    buffers: &mut [IoSliceMut<'_>],
) -> io::Result<usize> {
    let mut total = 0_usize;
    for buffer in buffers {
        if buffer.is_empty() {
            continue;
        }
        let count = match Input::read(reader, buffer) {
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
