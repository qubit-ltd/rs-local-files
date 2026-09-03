// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
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
    resource
        .parent()
        .and_then(|sandbox| sandbox.parent())
        .zip(resource.file_name())
        .map(|(parent, name)| parent.join(name))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid temporary resource sandbox path"))
}
