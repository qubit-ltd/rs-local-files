// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted temporary-sandbox cleanup failures.

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;
use std::path::PathBuf;

/// Retains a rooted temporary creation failure and failed sandbox cleanup.
#[derive(Debug)]
pub(crate) struct TempSandboxCleanupError {
    /// Native failure that caused the operation to stop.
    primary: io::Error,
    /// Sandbox path in the namespace exposed to the caller.
    sandbox: PathBuf,
    /// Native failure while removing the sandbox.
    cleanup: io::Error,
}

impl TempSandboxCleanupError {
    /// Creates an aggregate while preserving both native failures.
    pub(crate) const fn new(primary: io::Error, sandbox: PathBuf, cleanup: io::Error) -> Self {
        Self {
            primary,
            sandbox,
            cleanup,
        }
    }
}

impl Display for TempSandboxCleanupError {
    /// Formats the primary failure and the sandbox cleanup failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "{}; failed to remove temporary sandbox {}: {}",
            self.primary,
            self.sandbox.display(),
            self.cleanup,
        )
    }
}

impl Error for TempSandboxCleanupError {
    /// Returns the primary native creation failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;
    use std::path::PathBuf;

    use super::TempSandboxCleanupError;

    #[test]
    fn test_aggregate_display_and_source_preserve_both_failures() {
        let error = TempSandboxCleanupError::new(
            io::Error::from(io::ErrorKind::PermissionDenied),
            PathBuf::from("/sandbox-test"),
            io::Error::other("cleanup denied"),
        );
        let display = error.to_string();
        let primary = error.primary.to_string();
        assert!(display.contains(&primary));
        assert!(display.contains("/sandbox-test"));
        assert!(display.contains("cleanup denied"));
        assert_eq!(io::ErrorKind::PermissionDenied, error.primary.kind());
        assert_eq!(PathBuf::from("/sandbox-test"), error.sandbox);
        assert_eq!("cleanup denied", error.cleanup.to_string());
        assert_eq!(primary, error.source().expect("primary source").to_string());
    }
}
