// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use crate::LocalResult;
use crate::directory::LocalDirectoryHandle;
use crate::directory::binding::DirectoryPathBinding;
use crate::observation::LocalObservedReader;
use crate::options::LocalObservedReadOptions;

/// Opens a reader through this handle's retained root and original recipe.
pub(super) fn open(
    handle: &LocalDirectoryHandle,
    relative: &Path,
    options: &LocalObservedReadOptions,
) -> LocalResult<LocalObservedReader> {
    let mut reader = handle.filesystem().open_observed_reader_fixed(
        relative,
        &handle.diagnostic_path.join(relative),
        &handle.component_policy,
        options,
        handle.binding_deadline,
    )?;
    if let Some(anchors) = &handle.binding_anchors {
        reader = reader.with_binding(Box::new(DirectoryPathBinding::new(
            anchors.clone(),
            handle.binding_deadline,
        )));
    }
    Ok(reader)
}
