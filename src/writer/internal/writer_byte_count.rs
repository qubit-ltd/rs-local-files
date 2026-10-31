// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

/// Checked cumulative byte count for a writer stream.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct WriterByteCount(u64);

impl std::fmt::Debug for WriterByteCount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.0, formatter)
    }
}

impl WriterByteCount {
    #[cfg(test)]
    pub(crate) const fn with_initial_value(value: u64) -> Self {
        Self(value)
    }

    /// Returns the accumulated number of accepted bytes.
    #[inline]
    pub(crate) const fn get(self) -> u64 {
        self.0
    }

    /// Ensures a native write request can be accounted before issuing it.
    ///
    /// Returns `FileTooLarge` if the request cannot be represented as `u64`
    /// or would overflow the current count.
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
    pub(crate) fn check_vectored_request<'a>(&self, buffers: impl IntoIterator<Item = &'a [u8]>) -> io::Result<()> {
        let requested = checked_total_len(buffers.into_iter().map(<[u8]>::len))?;
        self.check_request(requested)
    }

    /// Adds the number actually accepted by a successful native write.
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

fn file_too_large() -> io::Error {
    io::Error::new(io::ErrorKind::FileTooLarge, "cumulative writer byte count exceeds u64")
}

/// Sums vectored buffer lengths, rejecting a `usize` overflow.
fn checked_total_len(lengths: impl IntoIterator<Item = usize>) -> io::Result<usize> {
    lengths.into_iter().try_fold(0usize, |total, length| {
        total.checked_add(length).ok_or_else(file_too_large)
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;

    use super::WriterByteCount;

    #[test]
    fn test_writer_byte_count_crosses_u32_and_records_actual_short_write() {
        let mut count = WriterByteCount::with_initial_value(u64::from(u32::MAX) - 1);
        count.check_request(3).expect("request fits u64");
        count.record_result(Ok(2)).expect("short write succeeds");
        assert_eq!(u64::from(u32::MAX) + 1, count.get());
    }

    #[test]
    fn test_writer_byte_count_zero_and_error_do_not_increase() {
        let mut count = WriterByteCount::with_initial_value(9);
        count.check_request(0).expect("zero request fits");
        count.record_result(Ok(0)).expect("zero write succeeds");
        let result: io::Result<usize> = Err(io::ErrorKind::Other.into());
        assert_eq!(io::ErrorKind::Other, count.record_result(result).unwrap_err().kind());
        assert_eq!(9, count.get());
    }

    #[test]
    fn test_writer_byte_count_rejects_u64_overflow_before_backend() {
        let count = WriterByteCount::with_initial_value(u64::MAX - 1);
        let called = Cell::new(false);
        let result = count.check_request(2);
        if result.is_ok() {
            called.set(true);
        }
        assert_eq!(io::ErrorKind::FileTooLarge, result.unwrap_err().kind());
        assert!(!called.get());
    }

    #[test]
    fn test_writer_byte_count_rejects_vectored_length_overflow_before_backend() {
        let count = WriterByteCount::with_initial_value(u64::MAX);
        let called = Cell::new(false);
        let result = count.check_vectored_request([b"x".as_slice()]);
        if result.is_ok() {
            called.set(true);
        }
        assert_eq!(io::ErrorKind::FileTooLarge, result.unwrap_err().kind());
        assert!(!called.get());
    }

    #[test]
    fn test_writer_byte_count_rejects_input_length_sum_overflow() {
        let error = super::checked_total_len([usize::MAX, 1]).expect_err("length sum overflows usize");
        assert_eq!(io::ErrorKind::FileTooLarge, error.kind());
    }

    #[test]
    fn test_writer_byte_count_debug_and_file_too_large_kind() {
        let count = WriterByteCount::with_initial_value(7);
        assert_eq!("7", format!("{count:?}"));
        assert_eq!(io::ErrorKind::FileTooLarge, super::file_too_large().kind());
    }
}
