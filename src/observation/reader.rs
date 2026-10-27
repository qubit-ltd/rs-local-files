//! Same-handle observed readers and bounded collection.

use std::io;

use crate::error::{LocalFileError, LocalFileErrorKind, LocalFileOperation};
use crate::observation::{
    LocalFileObservation, LocalObservationRequirement, LocalSourceVerification,
};
use crate::{LocalFileReader, LocalResult};

const BLOCK_SIZE: usize = 8 * 1024;

/// Reads the complete input subject to an optional byte maximum.
///
/// At the maximum, performs one one-byte probe to distinguish EOF from growth.
/// An input error or allocation failure returns an error and discards the
/// collected vector; bytes already consumed from the input cannot be restored.
pub(crate) fn collect_limited<S: qubit_io::ByteInput + ?Sized>(
    input: &mut S,
    maximum: Option<u64>,
) -> LocalResult<Vec<u8>> {
    collect_limited_with_reserver(input, maximum, |output, additional| {
        output.try_reserve(additional).map_err(io::Error::other)
    })
}

fn collect_limited_with_reserver<S, F>(
    input: &mut S,
    maximum: Option<u64>,
    mut reserve: F,
) -> LocalResult<Vec<u8>>
where
    S: qubit_io::ByteInput + ?Sized,
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
            match qubit_io::Input::read(input, &mut byte) {
                Ok(0) => return Ok(output),
                Ok(_) => return Err(resource_limit()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(read_error(error)),
            }
        }
        let new_len = output
            .len()
            .checked_add(request)
            .ok_or_else(resource_limit)?;
        let additional = new_len.saturating_sub(output.len());
        reserve(&mut output, additional).map_err(|_| resource_limit())?;
        let start = output.len();
        output.resize(new_len, 0);
        let read = loop {
            match qubit_io::Input::read(input, &mut output[start..]) {
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

#[cfg(test)]
pub(crate) fn collect_limited_with_test_reserver<S, F>(
    input: &mut S,
    maximum: Option<u64>,
    reserve: F,
) -> LocalResult<Vec<u8>>
where
    S: qubit_io::ByteInput + ?Sized,
    F: FnMut(&mut Vec<u8>, usize) -> io::Result<()>,
{
    collect_limited_with_reserver(input, maximum, reserve)
}

fn resource_limit() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read)
}

fn read_error(error: io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::Read, None, None, error)
}

pub(crate) trait ReaderPathBinding: std::fmt::Debug + Send + Sync {
    fn verify(&self) -> LocalResult<LocalSourceVerification>;
}

/// A byte reader whose initial and refreshed observations share its open handle.
#[derive(Debug)]
pub struct LocalObservedReader {
    reader: LocalFileReader,
    observation: LocalFileObservation,
    requirement: LocalObservationRequirement,
    binding: Option<Box<dyn ReaderPathBinding>>,
}

impl LocalObservedReader {
    pub(crate) fn from_reader(
        reader: LocalFileReader,
        requirement: LocalObservationRequirement,
    ) -> LocalResult<Self> {
        let observation = super::query::query_observation(reader.as_file(), requirement)?;
        Ok(Self {
            reader,
            observation,
            requirement,
            binding: None,
        })
    }

    pub(crate) fn with_binding(mut self, binding: Box<dyn ReaderPathBinding>) -> Self {
        self.binding = Some(match self.binding.take() {
            Some(existing) => Box::new(CombinedPathBinding { bindings: vec![existing, binding] }),
            None => binding,
        });
        self
    }

    /// Returns the observation captured when this reader was constructed.
    pub fn observation(&self) -> &LocalFileObservation {
        &self.observation
    }

    /// Queries the retained file handle again using the initial requirement.
    ///
    /// This does not change the value returned by [`Self::observation`].
    pub fn observe_now(&self) -> LocalResult<LocalFileObservation> {
        super::query::query_observation(self.reader.as_file(), self.requirement)
    }

    /// Checks the optional path binding retained when opening this reader.
    ///
    /// Returns `InvalidState` when no binding was requested. Native query
    /// failures are propagated; changed or missing names are reported as a
    /// successful [`LocalSourceVerification`] value.
    pub fn verify_path_binding(&self) -> LocalResult<LocalSourceVerification> {
        self.binding
            .as_ref()
            .ok_or_else(|| {
                LocalFileError::new(
                    LocalFileErrorKind::InvalidState,
                    LocalFileOperation::Metadata,
                )
            })?
            .verify()
    }

    /// Reads through EOF, failing if more than `maximum` bytes are available.
    ///
    /// At exactly the limit, one extra byte may be consumed to distinguish
    /// EOF from growth. Read and allocation errors do not return partial data.
    pub fn read_to_end_limited(&mut self, maximum: u64) -> LocalResult<Vec<u8>> {
        collect_limited(self, Some(maximum))
    }
}

#[derive(Debug)]
struct CombinedPathBinding {
    bindings: Vec<Box<dyn ReaderPathBinding>>,
}

impl ReaderPathBinding for CombinedPathBinding {
    fn verify(&self) -> LocalResult<LocalSourceVerification> {
        let mut missing = false;
        for binding in &self.bindings {
            match binding.verify()? {
                LocalSourceVerification::Changed => return Ok(LocalSourceVerification::Changed),
                LocalSourceVerification::Missing => missing = true,
                LocalSourceVerification::Unchanged => {}
            }
        }
        Ok(if missing { LocalSourceVerification::Missing } else { LocalSourceVerification::Unchanged })
    }
}

impl qubit_io::Input for LocalObservedReader {
    type Item = u8;

    fn read_vectored(&mut self, buffers: &mut [&mut [u8]]) -> io::Result<usize> {
        qubit_io::Input::read_vectored(&mut self.reader, buffers)
    }

    unsafe fn read_unchecked(
        &mut self,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> io::Result<usize> {
        // The Input caller guarantees this range; LocalFileReader applies the same contract.
        unsafe { qubit_io::Input::read_unchecked(&mut self.reader, output, index, count) }
    }
}
