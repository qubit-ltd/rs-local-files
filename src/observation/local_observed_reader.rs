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
/// Reservation failures are converted to `ResourceLimit`; the input may
/// already have been consumed when a later read or reservation fails.
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
fn collect_limited_with_reserver<S, F>(input: &mut S, maximum: Option<u64>, mut reserve: F) -> LocalResult<Vec<u8>>
where
    S: ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    let mut output = Vec::new();
    let mut count = 0_u64;
    loop {
        let (request, sentinel) = match maximum {
            Some(limit) if count == limit => (1, true),
            Some(limit) => {
                let remaining = limit - count;
                let available = usize::try_from(remaining).unwrap_or(usize::MAX);
                (BLOCK_SIZE.min(available.max(1)), false)
            }
            None => (BLOCK_SIZE, false),
        };
        if sentinel {
            let mut byte = [0_u8; 1];
            match Input::read(input, &mut byte) {
                Ok(0) => return Ok(output),
                Ok(_) => return Err(resource_limit()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(read_error(error)),
            }
        }
        let new_len = output.len().checked_add(request).ok_or_else(resource_limit)?;
        let additional = new_len.saturating_sub(output.len());
        reserve(&mut output, additional).map_err(|_| resource_limit())?;
        let start = output.len();
        output.resize(new_len, 0);
        let read = loop {
            match Input::read(input, &mut output[start..]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => break result,
            }
        };
        match read {
            Ok(0) => {
                output.truncate(start);
                return Ok(output);
            }
            Ok(read) => {
                if read > request {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::DataCorruption,
                        LocalFileOperation::Read,
                    ));
                }
                output.truncate(start + read);
                count = count
                    .checked_add(u64::try_from(read).map_err(|_| resource_limit())?)
                    .ok_or_else(resource_limit)?;
                if maximum.is_some_and(|limit| count > limit) {
                    return Err(resource_limit());
                }
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
