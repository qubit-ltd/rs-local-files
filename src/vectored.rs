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

/// Reserves descriptor storage and maps allocation failures to `OutOfMemory`.
///
/// # Type Parameters
///
/// - `T`: the type stored in the descriptor vector.
///
/// # Parameters
///
/// - `slices`: the vector whose descriptor storage is reserved.
/// - `capacity`: the additional number of descriptors to reserve.
fn reserve_slices<T>(slices: &mut Vec<T>, capacity: usize) -> io::Result<()> {
    slices
        .try_reserve(capacity)
        .map_err(|error| io::Error::new(io::ErrorKind::OutOfMemory, error))
}

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
    reserve_slices(&mut slices, buffers.len())?;
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
#[cfg_attr(unix, allow(dead_code))]
pub(crate) fn byte_slices_mut<'a>(buffers: &'a mut [IoSliceMut<'_>]) -> io::Result<Vec<&'a mut [u8]>> {
    let mut slices = Vec::new();
    reserve_slices(&mut slices, buffers.len())?;
    slices.extend(buffers.iter_mut().map(|buffer| &mut buffer[..]));
    Ok(slices)
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    #[test]
    fn test_slice_descriptor_reservation_maps_capacity_errors_to_out_of_memory() {
        let mut slices = Vec::<u8>::new();

        let error =
            super::reserve_slices(&mut slices, usize::MAX).expect_err("an impossible descriptor capacity should fail");

        assert_eq!(error.kind(), ErrorKind::OutOfMemory);
        assert!(error.get_ref().is_some(), "the allocation cause should be retained");
    }
}
