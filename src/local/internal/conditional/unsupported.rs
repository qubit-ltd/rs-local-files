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
pub(crate) fn open_host(_path: &Path, _options: &LocalWriteOptions) -> io::Result<()> {
    Err(unsupported_error())
}

/// Returns a fail-closed error for rooted conditional writes on unsupported
/// platforms.
pub(crate) fn open_rooted(
    _root: std::fs::File,
    _relative: &Path,
    _diagnostic: &Path,
    _options: &LocalWriteOptions,
) -> io::Result<()> {
    Err(unsupported_error())
}

fn unsupported_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "conditional writes are unsupported on this platform",
    )
}

#[cfg(test)]
mod tests {
    use super::{open_host, open_rooted};
    use crate::options::{LocalWriteMode, LocalWriteOptions};

    #[test]
    fn unsupported_conditional_constructors_fail_closed() {
        let options = LocalWriteOptions::new(LocalWriteMode::CreateNew);
        assert_eq!(
            open_host(std::path::Path::new("target"), &options)
                .expect_err("Host construction must fail closed")
                .kind(),
            std::io::ErrorKind::Unsupported
        );
        assert_eq!(
            open_rooted(
                std::fs::File::open(".").expect("current directory should open"),
                std::path::Path::new("target"),
                std::path::Path::new("target"),
                &options,
            )
            .expect_err("Rooted construction must fail closed")
            .kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
