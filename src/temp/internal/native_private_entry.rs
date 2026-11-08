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
    /// Keeps the containing directory open so cleanup can address the entry
    /// relative to that stable directory handle.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) parent: Arc<File>,
    /// Stores the entry's native name relative to `parent`, without converting
    /// it to a potentially lossy UTF-8 representation.
    #[cfg_attr(windows, allow(dead_code))]
    pub(crate) name: OsString,
    /// Retains an open handle to the created object when the creation path
    /// obtained one; it can be absent when opening the object failed.
    pub(crate) file: Option<Arc<File>>,
    /// Records the object's observed identity when it was available, allowing
    /// cleanup to avoid removing a replacement entry.
    pub(crate) identity: Option<LocalFileIdentity>,
    /// Indicates whether cleanup must remove a directory rather than a file.
    pub(crate) directory: bool,
}
