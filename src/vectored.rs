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

pub(crate) fn byte_slices<'a>(buffers: &'a [IoSlice<'_>]) -> io::Result<Vec<&'a [u8]>> {
    let mut slices = Vec::new();
    slices
        .try_reserve(buffers.len())
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    slices.extend(buffers.iter().map(|buffer| &buffer[..]));
    Ok(slices)
}

pub(crate) fn byte_slices_mut<'a>(buffers: &'a mut [IoSliceMut<'_>]) -> io::Result<Vec<&'a mut [u8]>> {
    let mut slices = Vec::new();
    slices
        .try_reserve(buffers.len())
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    slices.extend(buffers.iter_mut().map(|buffer| &mut buffer[..]));
    Ok(slices)
}

#[cfg(windows)]
pub(crate) fn io_slices_mut<'a>(buffers: &'a mut [&mut [u8]]) -> io::Result<Vec<IoSliceMut<'a>>> {
    let mut slices = Vec::new();
    slices
        .try_reserve(buffers.len())
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))?;
    slices.extend(buffers.iter_mut().map(|buffer| IoSliceMut::new(buffer)));
    Ok(slices)
}
