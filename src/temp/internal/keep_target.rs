// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Generated publication targets for temporary resources.

use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Promotes `<parent>/<sandbox>/<resource>` to `<parent>/<resource>`.
///
/// The helper is purely lexical: authority-specific persistence performs all
/// filesystem access and path validation afterwards.
pub(crate) fn generated_target(resource: &Path) -> io::Result<PathBuf> {
    let sandbox = resource
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "temporary resource has no sandbox parent"))?;
    let parent = sandbox.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "temporary sandbox has no publication parent",
        )
    })?;
    let name = resource
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "temporary resource has no file name"))?;
    Ok(parent.join(name))
}
