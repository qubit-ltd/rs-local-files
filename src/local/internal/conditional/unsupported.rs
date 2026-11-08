// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unsupported-platform conditional backend entry point.

use std::io;
use std::path::Path;

use crate::options::LocalWriteOptions;

/// Returns a fail-closed error for platforms without a conditional backend.
///
/// T5's platform dispatch calls these constructors before creating a staging
/// file, so unsupported targets cannot accidentally fall back to an ordinary
/// writer or expose a partially initialized backend.
///
/// # Parameters
///
/// `path` and `options` identify the requested destination and write policy;
/// neither is inspected because this backend rejects every request.
///
/// # Returns
///
/// Always returns an error because this platform has no conditional backend.
///
/// # Errors
///
/// Returns [`io::ErrorKind::Unsupported`] for every request.
pub(crate) fn open_host(_path: &Path, _options: &LocalWriteOptions) -> io::Result<()> {
    Err(unsupported_error())
}

/// Returns a fail-closed error for rooted conditional writes on unsupported
/// platforms.
///
/// # Parameters
///
/// `root` is consumed as the rooted directory handle; `relative` and
/// `namespace_absolute` identify the destination, while `diagnostic` supplies
/// its optional error-reporting path. `options` describes the write policy.
/// These values are not inspected because this backend rejects every request.
///
/// # Returns
///
/// Always returns an error because this platform has no conditional backend.
///
/// # Errors
///
/// Returns [`io::ErrorKind::Unsupported`] for every request.
pub(crate) fn open_rooted(
    _root: std::fs::File,
    _relative: &Path,
    _namespace_absolute: &Path,
    _diagnostic: Option<&Path>,
    _options: &LocalWriteOptions,
) -> io::Result<()> {
    Err(unsupported_error())
}

/// Creates the shared error returned by every constructor in this backend.
///
/// The `Unsupported` kind lets callers distinguish an unavailable platform
/// capability from errors encountered while opening or writing a destination.
///
/// # Returns
///
/// Returns an I/O error with kind [`io::ErrorKind::Unsupported`].
fn unsupported_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "conditional writes are unsupported on this platform",
    )
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::ErrorKind;
    use std::path::Path;

    use super::open_host;
    use super::open_rooted;
    use crate::options::LocalWriteMode;
    use crate::options::LocalWriteOptions;

    #[test]
    fn test_unsupported_conditional_constructors_fail_closed() {
        let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
        assert_eq!(
            open_host(Path::new("target"), &options)
                .expect_err("Host construction must fail closed")
                .kind(),
            ErrorKind::Unsupported
        );
        assert_eq!(
            open_rooted(
                File::open(".").expect("current directory should open"),
                Path::new("target"),
                Path::new("target"),
                None,
                &options,
            )
            .expect_err("Rooted construction must fail closed")
            .kind(),
            ErrorKind::Unsupported
        );
    }
}
