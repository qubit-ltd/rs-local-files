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

#[cfg(test)]
mod tests {
    use super::RootedLocalFileSystem;
    use crate::SizeLimit;
    use crate::rooted::Root;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn authority_clone_failure_keeps_root_and_uses_unknown_limit_fallback() {
        let fixture = tempfile::tempdir().expect("root fixture");
        let root = Root::open(fixture.path()).expect("open root authority");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 1);

        let filesystem = RootedLocalFileSystem::from_root(root).expect("retain rooted filesystem");

        assert!(guard.fired());
        assert_eq!(filesystem.limits().max_path_length(), SizeLimit::Unknown);
        assert_eq!(filesystem.limits().max_component_length(), SizeLimit::Unknown);
        assert!(filesystem.root_authority().metadata().is_ok());
    }
}
