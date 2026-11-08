// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::io;

/// Checked cumulative byte count for a writer stream.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WriterByteCount(
    /// Number of bytes accepted by the writer so far.
    u64,
);

impl fmt::Debug for WriterByteCount {
    /// Formats the accumulated byte count as its underlying integer.
    ///
    /// # Parameters
    ///
    /// * `self` - The counter whose accumulated byte count is formatted.
    /// * `formatter` - The destination formatter supplied by the formatting
    ///   framework.
    ///
    /// # Returns
    ///
    /// The formatter's result.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing the integer representation
    /// fails.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, formatter)
    }
}

impl WriterByteCount {
    /// Creates a counter initialized to a chosen value for internal tests.
    ///
    /// # Parameters
    ///
    /// * `value` - The cumulative byte count to use as the initial state.
    ///
    /// # Returns
    ///
    /// A counter initialized with `value`.
    #[must_use]
    #[cfg(test)]
    pub(crate) const fn with_initial_value(value: u64) -> Self {
        Self(value)
    }

    /// Returns the accumulated number of accepted bytes.
    #[must_use]
    #[inline]
    pub(crate) const fn get(self) -> u64 {
        self.0
    }

    /// Ensures a native write request can be accounted before issuing it.
    ///
    /// # Parameters
    ///
    /// * `self` - The current cumulative byte count.
    /// * `requested` - Number of bytes the backend may accept in the request.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the request can be represented and added to the current
    /// count.
    ///
    /// # Errors
    ///
    /// Returns `FileTooLarge` if the request cannot be represented as `u64` or
    /// would overflow the current count.
    #[inline]
    pub(crate) fn check_request(&self, requested: usize) -> io::Result<()> {
        let requested = match u64::try_from(requested) {
            Ok(requested) => requested,
            Err(_) => return Err(file_too_large()),
        };
        self.0.checked_add(requested).ok_or_else(file_too_large)?;
        Ok(())
    }

    /// Checks an aggregate vectored request, including sum overflow.
    ///
    /// # Parameters
    ///
    /// * `self` - The current cumulative byte count.
    /// * `buffers` - The buffers whose combined length may be written.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the combined request can be accounted for.
    ///
    /// # Errors
    ///
    /// Returns `FileTooLarge` if the combined length or cumulative count
    /// overflows.
    pub(crate) fn check_vectored_request<'a>(&self, buffers: impl IntoIterator<Item = &'a [u8]>) -> io::Result<()> {
        let requested = checked_total_len(buffers.into_iter().map(<[u8]>::len))?;
        self.check_request(requested)
    }

    /// Adds the number actually accepted by a successful native write.
    ///
    /// # Parameters
    ///
    /// * `self` - The counter to update.
    /// * `written` - Number of bytes actually accepted by the backend.
    ///
    /// # Panics
    ///
    /// Panics if the resulting cumulative count exceeds `u64`; callers must
    /// check the full request before invoking the backend.
    #[inline]
    pub(crate) fn record_written(&mut self, written: usize) {
        self.0 = self
            .0
            .checked_add(u64::try_from(written).expect("accepted write count fits u64"))
            .expect("write request was checked before calling the backend");
    }

    /// Records a successful write result while preserving errors unchanged.
    ///
    /// # Parameters
    ///
    /// * `self` - The counter to update when the write succeeded.
    /// * `result` - The backend result, including the actual accepted byte
    ///   count on success.
    ///
    /// # Returns
    ///
    /// The successful byte count after recording it, or the original backend
    /// error.
    ///
    /// # Errors
    ///
    /// Returns the backend error unchanged when the write failed.
    #[inline]
    pub(crate) fn record_result(&mut self, result: io::Result<usize>) -> io::Result<usize> {
        match result {
            Ok(written) => {
                self.record_written(written);
                Ok(written)
            }
            Err(error) => Err(error),
        }
    }
}

/// Creates the error used when a byte count cannot be represented safely.
///
/// # Returns
///
/// An `io::Error` with kind `FileTooLarge`.
#[must_use]
fn file_too_large() -> io::Error {
    io::Error::new(io::ErrorKind::FileTooLarge, "cumulative writer byte count exceeds u64")
}

/// Sums vectored buffer lengths, rejecting a `usize` overflow.
///
/// # Parameters
///
/// * `lengths` - Buffer lengths to add without exceeding `usize`.
///
/// # Returns
///
/// The sum of the lengths when representable.
///
/// # Errors
///
/// Returns `FileTooLarge` if their sum overflows `usize`.
fn checked_total_len(lengths: impl IntoIterator<Item = usize>) -> io::Result<usize> {
    lengths.into_iter().try_fold(0usize, |total, length| {
        total.checked_add(length).ok_or_else(file_too_large)
    })
}

#[cfg(test)]
mod tests {
    use std::io;

    #[test]
    fn test_writer_byte_count_rejects_input_length_sum_overflow() {
        let error = super::checked_total_len([usize::MAX, 1]).expect_err("length sum overflows usize");
        assert_eq!(io::ErrorKind::FileTooLarge, error.kind());
    }

    #[test]
    fn test_writer_byte_count_file_too_large_kind() {
        assert_eq!(io::ErrorKind::FileTooLarge, super::file_too_large().kind());
    }
}
