// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recoverable temporary-resource persistence errors.

use std::error::Error;
use std::fmt::{
    Debug,
    Display,
    Formatter,
    Result as FmtResult,
};
use std::io;

/// Persistence error that returns ownership of the temporary resource.
#[derive(Debug)]
pub struct LocalPersistError<T> {
    /// Native I/O error that prevented persistence.
    pub error: io::Error,
    /// Temporary resource retained after the failed operation.
    pub resource: T,
}

impl<T> LocalPersistError<T> {
    /// Creates a recoverable persistence error.
    ///
    /// # Parameters
    /// - `error`: Native I/O error that prevented persistence.
    /// - `resource`: Temporary resource retained after the failure.
    ///
    /// # Returns
    /// New persistence error owning both values.
    pub(crate) fn new(error: io::Error, resource: T) -> Self {
        Self { error, resource }
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained native error.
    #[inline]
    pub fn kind(&self) -> io::ErrorKind {
        self.error.kind()
    }

    /// Splits this error into its native error and temporary resource.
    ///
    /// # Returns
    /// Native I/O error followed by the retained temporary resource.
    #[inline]
    pub fn into_parts(self) -> (io::Error, T) {
        (self.error, self.resource)
    }
}

impl<T> Display for LocalPersistError<T> {
    /// Formats the retained native error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "failed to persist temporary resource: {}",
            self.error
        )
    }
}

impl<T> Error for LocalPersistError<T>
where
    T: Debug,
{
    /// Returns the retained native I/O error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
