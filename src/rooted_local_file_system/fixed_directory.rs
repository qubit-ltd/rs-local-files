// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use super::RootedLocalFileSystem;
use crate::LocalFileSystemCapabilities;
use crate::LocalFileSystemLimits;
use crate::LocalPathLengthUnit;
use crate::LocalResult;
use crate::SizeLimit;

impl RootedLocalFileSystem {
    pub(crate) fn root_authority(&self) -> &crate::rooted::Root {
        &self.root
    }

    pub(crate) fn from_root(root: crate::rooted::Root) -> LocalResult<Self> {
        let root = Arc::new(root);
        let limits = root
            .try_clone_authority()
            .and_then(|file| crate::capability::probe_limits(&file))
            .unwrap_or_else(|_| {
                LocalFileSystemLimits::new(SizeLimit::Unknown, SizeLimit::Unknown, LocalPathLengthUnit::native())
            });
        Ok(Self {
            root,
            capabilities: LocalFileSystemCapabilities::detect_rooted(),
            limits,
        })
    }
}
