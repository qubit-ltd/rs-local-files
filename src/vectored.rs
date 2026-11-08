// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::io::IoSlice;
use std::io::IoSliceMut;

/// Borrows the byte slices in an ordered array of I/O buffers.
///
/// The returned descriptors preserve `buffers` order and borrow the original
/// storage for `'a`. This allocates storage for slice descriptors only; it does
/// not copy any file contents. Allocation failure is returned as an
/// [`io::ErrorKind::OutOfMemory`] error.
///
/// # Parameters
///
/// - `buffers`: the ordered immutable I/O buffers to borrow.
///
/// # Returns
///
/// A vector containing one borrowed byte slice for each input buffer.
///
/// # Errors
///
/// Returns `OutOfMemory` if storage for the slice descriptors cannot be
/// reserved.
pub(crate) fn byte_slices<'a>(buffers: &'a [IoSlice<'_>]) -> io::Result<Vec<&'a [u8]>> {
    let mut slices = Vec::new();
    slices
        .try_reserve(buffers.len())
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    slices.extend(buffers.iter().map(|buffer| &buffer[..]));
    Ok(slices)
}

/// Mutably borrows the byte slices in an ordered array of I/O buffers.
///
/// The returned descriptors preserve `buffers` order and borrow the original
/// storage for `'a`; those mutable borrows remain exclusive while the returned
/// vector is alive. This allocates storage for slice descriptors only; it does
/// not copy any file contents. Allocation failure is returned as an
/// [`io::ErrorKind::OutOfMemory`] error.
///
/// # Parameters
///
/// - `buffers`: the ordered mutable I/O buffers to borrow.
///
/// # Returns
///
/// A vector containing one mutably borrowed byte slice for each input buffer.
///
/// # Errors
///
/// Returns `OutOfMemory` if storage for the slice descriptors cannot be
/// reserved.
pub(crate) fn byte_slices_mut<'a>(buffers: &'a mut [IoSliceMut<'_>]) -> io::Result<Vec<&'a mut [u8]>> {
    let mut slices = Vec::new();
    slices
        .try_reserve(buffers.len())
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    slices.extend(buffers.iter_mut().map(|buffer| &mut buffer[..]));
    Ok(slices)
}
