// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Same-handle observed readers and bounded collection.

#[path = "local_observed_reader/internal/combined_path_binding.rs"]
mod combined_path_binding;
#[path = "local_observed_reader/local_observed_reader.rs"]
mod local_observed_reader_type;
#[path = "local_observed_reader/reader_path_binding.rs"]
mod reader_path_binding;

use std::io;

use qubit_io::ByteInput;
use qubit_io::Input;

pub use self::local_observed_reader_type::LocalObservedReader;
pub(crate) use self::reader_path_binding::ReaderPathBinding;
use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::error::LocalReadLimitError;

const BLOCK_SIZE: usize = 8 * 1024;

/// Reads the complete input subject to an optional byte maximum.
///
/// At the maximum, performs one one-byte probe to distinguish EOF from growth.
/// An input error or allocation failure returns an error and discards the
/// collected vector; bytes already consumed from the input cannot be restored.
///
/// # Type Parameters
/// - `S`: Byte input implementation, which may be dynamically sized.
///
/// # Parameters
/// - `input`: Input stream consumed until EOF or the configured limit.
/// - `maximum`: Maximum byte count; `None` collects until EOF without a limit.
///
/// # Returns
/// The collected bytes when EOF occurs within the limit.
///
/// # Errors
/// Returns a read error or `ResourceLimit` when the input exceeds the limit or
/// memory reservation fails.
pub(crate) fn collect_limited<S: ByteInput + ?Sized>(input: &mut S, maximum: Option<u64>) -> LocalResult<Vec<u8>> {
    collect_limited_with_reserver(input, maximum, |output, additional| {
        output.try_reserve(additional).map_err(io::Error::other)
    })
}

/// Collects bytes while using a caller-supplied reservation strategy.
///
/// Reads into a fixed stack buffer before reserving output capacity. The input
/// may already have been consumed when reservation fails.
///
/// # Type Parameters
/// - `S`: Byte input implementation, which may be dynamically sized.
/// - `F`: Reservation callback type.
///
/// # Parameters
/// - `input`: Input stream consumed by the collector.
/// - `maximum`: Maximum byte count; `None` collects until EOF without a limit.
/// - `reserve`: Callback that reserves capacity before bytes are appended.
///
/// # Returns
/// The collected bytes when EOF occurs within the limit.
///
/// # Errors
/// Returns a read error or `ResourceLimit` when the input exceeds the limit or
/// the callback cannot reserve capacity.
fn collect_limited_with_reserver<S, F>(input: &mut S, maximum: Option<u64>, mut reserve: F) -> LocalResult<Vec<u8>>
where
    S: ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    let mut output = Vec::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; BLOCK_SIZE];
    loop {
        let (request, sentinel) = match maximum {
            Some(limit) if count == limit => (1, true),
            Some(limit) => {
                let remaining = limit - count;
                let available = usize::try_from(remaining).unwrap_or(usize::MAX);
                (BLOCK_SIZE.min(available), false)
            }
            None => (BLOCK_SIZE, false),
        };
        if sentinel {
            match Input::read(input, &mut buffer[..1]) {
                Ok(0) => return Ok(output),
                Ok(_) => {
                    let observed = count.checked_add(1).ok_or_else(length_overflow)?;
                    return Err(read_limit(maximum.expect("sentinel requires a limit"), observed));
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(read_error(error)),
            }
        }
        let read = loop {
            match Input::read(input, &mut buffer[..request]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        match read {
            Ok(0) => return Ok(output),
            Ok(read) => {
                if read > request {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::DataCorruption,
                        LocalFileOperation::Read,
                    ));
                }
                let new_count = count
                    .checked_add(u64::try_from(read).map_err(|_| length_overflow())?)
                    .ok_or_else(length_overflow)?;
                let new_len = output.len().checked_add(read).ok_or_else(length_overflow)?;
                reserve(&mut output, read).map_err(allocation_error)?;
                output.extend_from_slice(&buffer[..read]);
                count = new_count;
                if let Some(limit) = maximum
                    && count > limit
                {
                    return Err(read_limit(limit, count));
                }
                debug_assert_eq!(output.len(), new_len);
            }
            Err(error) => return Err(read_error(error)),
        }
    }
}

/// Exposes the reservation callback so tests can exercise allocation failures.
///
/// # Type Parameters
/// - `S`: Byte input implementation, which may be dynamically sized.
/// - `F`: Reservation callback type.
///
/// # Parameters
/// - `input`: Input stream consumed by the collector.
/// - `maximum`: Maximum byte count; `None` collects until EOF without a limit.
/// - `reserve`: Callback that reserves capacity before the next read.
///
/// # Returns
/// The collected bytes when EOF occurs within the limit.
///
/// # Errors
/// Returns a read error or `ResourceLimit` when the input exceeds the limit or
/// the callback cannot reserve capacity.
#[cfg(test)]
pub(crate) fn collect_limited_with_test_reserver<S, F>(
    input: &mut S,
    maximum: Option<u64>,
    reserve: F,
) -> LocalResult<Vec<u8>>
where
    S: ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    collect_limited_with_reserver(input, maximum, reserve)
}

/// Creates the read-operation error used when a byte limit or allocation is
/// exceeded.
///
/// # Returns
/// A `ResourceLimit` error classified as a read operation.
fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

/// Creates typed facts for an observed byte-limit violation.
///
/// # Parameters
/// - `limit`: Configured maximum byte count.
/// - `observed`: Observed length or proven lower bound.
///
/// # Returns
/// A structured `ResourceLimit` error with typed read-limit facts.
fn read_limit(limit: u64, observed: u64) -> LocalFileError {
    LocalFileError::from_read_limit(
        LocalFileOperation::Read,
        None,
        LocalReadLimitError::new(limit, observed),
    )
}

/// Preserves the allocator error while keeping the stable resource category.
///
/// # Parameters
/// - `source`: Error returned while reserving output capacity.
///
/// # Returns
/// A `ResourceLimit` error retaining the original source.
fn allocation_error(source: io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::Read, None, None, source)
        .with_kind(LocalFileErrorKind::ResourceLimit)
        .with_reason("unable to reserve memory for read data")
}

/// Reports arithmetic overflow without describing it as an input-limit breach.
fn length_overflow() -> LocalFileError {
    resource_limit().with_reason("read length exceeds addressable capacity")
}

/// Converts a native read error while retaining it as the structured source.
///
/// # Parameters
/// - `error`: Native read error to retain.
///
/// # Returns
/// A structured read error containing the native source.
fn read_error(error: io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::Read, None, None, error)
}

#[cfg(test)]
mod tests {
    use super::length_overflow;
    use crate::error::LocalFileErrorKind;

    #[test]
    fn test_read_length_overflow_is_not_a_typed_byte_limit() {
        let error = length_overflow();

        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
        assert!(error.read_limit_error().is_none());
        assert_eq!(error.reason(), Some("read length exceeds addressable capacity"));
    }
}
