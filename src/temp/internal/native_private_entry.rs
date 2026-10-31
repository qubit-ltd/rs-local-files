// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsString;
use std::fs::File;
use std::sync::Arc;

use crate::observation::LocalFileIdentity;

/// Native private resource retained after a creation step partially succeeds.
///
/// `parent` and `name` identify the entry relative to a retained directory
/// handle; `file` retains a handle to the created object when available.
/// `identity` records an observed identity when observation succeeded, and
/// `directory` selects file versus directory cleanup behavior. Moving this
/// value transfers recovery ownership; it does not promise that cleanup will
/// succeed.
#[derive(Debug)]
pub(crate) struct NativePrivateEntry {
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) parent: Arc<File>,
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) name: OsString,
    pub(crate) file: Option<Arc<File>>,
    pub(crate) identity: Option<LocalFileIdentity>,
    pub(crate) directory: bool,
}
