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

#[cfg(test)]
mod tests {
    use std::io::IoSlice;
    use std::io::IoSliceMut;

    use super::byte_slices;
    use super::byte_slices_mut;

    #[test]
    fn byte_slices_preserve_buffer_order_and_contents() {
        let buffers = [IoSlice::new(b"first"), IoSlice::new(b"second")];
        let slices = byte_slices(&buffers).expect("borrowed slices");
        assert_eq!(slices, [b"first".as_slice(), b"second".as_slice()]);
    }

    #[test]
    fn byte_slices_mut_preserve_mutable_buffers_in_order() {
        let mut first = *b"first";
        let mut second = *b"second";
        let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];
        let mut slices = byte_slices_mut(&mut buffers).expect("mutable borrowed slices");
        slices[0][0] = b'F';
        slices[1][0] = b'S';
        drop(slices);
        assert_eq!(&first, b"First");
        assert_eq!(&second, b"Second");
    }
}
