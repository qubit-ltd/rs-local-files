// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Implements rooted file and directory copy operations.
#[cfg(feature = "digest")]
mod conditional;
mod copy;
// Implements rooted deletion operations.
mod delete;
// Implements rooted directory operations.
mod directory;
#[path = "rooted_local_file_system/fixed_directory.rs"]
mod fixed_directory;
mod temp_sandbox_cleanup_error;
// Implements rooted metadata, reader, and writer operations.
#[path = "rooted_local_file_system/io.rs"]
mod io_operations;
// Implements rooted metadata conversion operations.
#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
// Provides shared rooted path and I/O helpers.
mod path_support;
// Implements rooted rename operations.
mod rename;
// Describes rooted path-resolution steps.
mod resolution_step;
// Provides rooted authority and validation helpers.
mod support;
// Tracks rooted symbolic-link identity state.
mod symlink_identity;
// Implements rooted temporary-resource operations.
mod temp;

pub(super) use std::io;
pub(super) use std::path::Path;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;

pub(crate) use path_support::rooted_destination_is_directory;
pub(crate) use path_support::rooted_io_error;
pub(crate) use path_support::rooted_metadata;
pub(crate) use path_support::rooted_path;
pub(crate) use path_support::rooted_temp_parent;
pub(crate) use path_support::temp_candidate;
pub(crate) use path_support::validate_rooted_temp_parent;
pub(crate) use support::resolve_rooted_path;
pub(crate) use support::resolve_rooted_path_allow_root;
pub(crate) use support::resolve_rooted_path_allow_root_with_component_policy_and_budget;
pub(crate) use support::resolve_rooted_path_allow_root_with_deadline;
pub(super) use support::sync_rooted_copy_parent_chain;
pub(super) use support::validate_rooted_list_start;

pub(super) use crate::LocalCopyFailure;
pub(super) use crate::LocalCopyMethod;
pub(super) use crate::LocalCopyOptions;
pub(super) use crate::LocalCopyOutcome;
pub(super) use crate::LocalCopyResult;
pub(super) use crate::LocalCopyStats;
pub(super) use crate::LocalCreateDirectoryOptions;
pub(super) use crate::LocalCreateDirectoryOutcome;
pub(super) use crate::LocalDeleteOptions;
pub(super) use crate::LocalDeleteOutcome;
pub(super) use crate::LocalDirectoryWalker;
pub(super) use crate::LocalFileError;
pub(super) use crate::LocalFileErrorKind;
pub(super) use crate::LocalFileOperation;
pub(super) use crate::LocalFileReader;
pub(super) use crate::LocalFileSystemCapabilities;
pub(super) use crate::LocalFileSystemLimits;
pub(super) use crate::LocalFileWriter;
pub(super) use crate::LocalListOptions;
pub(super) use crate::LocalReadOptions;
pub(super) use crate::LocalRenameOptions;
pub(super) use crate::LocalRenameOutcome;
pub(super) use crate::LocalRenameResult;
pub(super) use crate::LocalResult;
pub(super) use crate::LocalSymlinkPolicy;
pub(super) use crate::LocalTempDirectory;
pub(super) use crate::LocalTempDirectoryOptions;
pub(super) use crate::LocalTempFile;
pub(super) use crate::LocalTempFileOptions;
pub(super) use crate::LocalWriteMode;
pub(super) use crate::LocalWriteOptions;
pub(super) use crate::local::copy_failure_published;
pub(super) use crate::local::copy_failure_unchanged;
pub(super) use crate::local::ensure_required_directory_durability;
pub(super) use crate::local::published_durability;
pub(super) use crate::local::rename_failure_after_native_attempt;
pub(super) use crate::local::rename_failure_renamed;
pub(super) use crate::local::rename_failure_unchanged;
pub(super) use crate::local::validate_temp_affixes;

/// Descriptor- or handle-relative authority for one opened native directory.
#[derive(Clone, Debug)]
pub(crate) struct RootedLocalFileSystem {
    /// The single opened descriptor or handle authority.
    root: Arc<crate::rooted::Root>,
    /// Capability snapshot cached when the authority is opened.
    capabilities: LocalFileSystemCapabilities,
    /// Best-effort path limits captured from the opened root authority.
    limits: LocalFileSystemLimits,
}

impl RootedLocalFileSystem {
    /// Opens a native root descriptor or handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path to anchor.
    ///
    /// # Returns
    ///
    /// A rooted authority whose later operations do not rely on path lookup.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the directory cannot be bound and opened
    /// securely or the current platform lacks rooted primitives.
    #[allow(dead_code)] // Preserved as the public unrestricted-opening API.
    pub fn open(path: &Path) -> LocalResult<Self> {
        Self::open_with_options(path, &crate::options::LocalRootOpenOptions::new())
    }

    pub(crate) fn open_with_options(path: &Path, options: &crate::options::LocalRootOpenOptions) -> LocalResult<Self> {
        let mut contract = crate::resolution::ResolutionContract::unrestricted().tightened(options.limits());
        if let Some(policy) = options.resolution_policy() {
            contract = contract.with_policy(policy.clone());
        }
        Self::open_with_contract(
            path,
            contract,
            options
                .symlink_policy()
                .unwrap_or(LocalSymlinkPolicy::FollowWithinScope),
        )
    }

    pub(crate) fn open_with_contract(
        path: &Path,
        contract: crate::resolution::ResolutionContract,
        policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        Self::open_with_contract_and_deadline(path, contract, policy, None)
    }

    pub(crate) fn open_with_contract_and_deadline(
        path: &Path,
        contract: crate::resolution::ResolutionContract,
        policy: LocalSymlinkPolicy,
        deadline: Option<std::time::Instant>,
    ) -> LocalResult<Self> {
        support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
        if policy == LocalSymlinkPolicy::FollowAcrossScope {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf()),
            );
        }
        #[cfg(unix)]
        let root = {
            let absolute = std::path::absolute(path).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let host = crate::rooted::Root::open(Path::new("/"))
                .map_err(|source| {
                    LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                })?
                .with_resolution_contract(contract.clone());
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let relative = absolute.strip_prefix("/").unwrap_or(&absolute);
            let resolved = support::resolve_rooted_path_allow_root_with_scope_and_deadline(
                &host,
                relative,
                policy,
                true,
                LocalFileOperation::OpenRoot,
                crate::directory::LocalResolutionScope::OpeningRoot,
                deadline,
            )?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let mut cursor = host.resolution_cursor().map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            for component in resolved.components() {
                support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
                cursor.descend(component.as_os_str()).map_err(|source| {
                    LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                })?;
                support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            }
            crate::rooted::Root::from_directory_authority_with_contract(cursor.into_file(), absolute, contract)
        };
        #[cfg(windows)]
        let root = {
            use std::path::Component;
            let absolute = std::path::absolute(path).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let mut components = absolute.components();
            let prefix = match components.next() {
                Some(Component::Prefix(prefix)) => prefix,
                _ => {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                            .with_path(path.to_path_buf()),
                    );
                }
            };
            let mut anchor = PathBuf::new();
            anchor.push(prefix.as_os_str());
            if !matches!(components.next(), Some(Component::RootDir)) {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                        .with_path(path.to_path_buf()),
                );
            }
            anchor.push(std::path::MAIN_SEPARATOR_STR);
            let host = crate::rooted::Root::open(&anchor)
                .map_err(|source| {
                    LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                })?
                .with_resolution_contract(contract.clone());
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let relative = absolute.strip_prefix(&anchor).map_err(|_| {
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf())
            })?;
            let resolved = support::resolve_rooted_path_allow_root_with_scope_and_deadline(
                &host,
                relative,
                policy,
                true,
                LocalFileOperation::OpenRoot,
                crate::directory::LocalResolutionScope::OpeningRoot,
                deadline,
            )?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            let mut cursor = host.resolution_cursor().map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            for component in resolved.components() {
                support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
                cursor.descend(component.as_os_str()).map_err(|source| {
                    LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                })?;
                support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
            }
            crate::rooted::Root::from_directory_authority_with_contract(cursor.into_file(), absolute, contract)
        };
        #[cfg(not(any(unix, windows)))]
        let root = {
            let root = crate::rooted::Root::open(path).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            root.with_resolution_contract(contract)
        };
        support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
        let root = Arc::new(root);
        support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
        let limits = match root
            .try_clone_authority()
            .and_then(|file| crate::capability::probe_limits(&file))
        {
            Ok(limits) => limits,
            Err(_) => LocalFileSystemLimits::new(
                crate::SizeLimit::Unknown,
                crate::SizeLimit::Unknown,
                crate::LocalPathLengthUnit::native(),
            ),
        };
        support::check_resolution_deadline(deadline, path, LocalFileOperation::OpenRoot)?;
        Ok(Self {
            root,
            capabilities: LocalFileSystemCapabilities::detect_rooted(),
            limits,
        })
    }

    /// Returns the non-authoritative diagnostic path captured at open time.
    #[must_use]
    #[inline]
    pub fn diagnostic_path(&self) -> &Path {
        self.root.path()
    }

    /// Returns the native capability snapshot cached for this opened authority.
    #[must_use = "inspect the rooted capability snapshot"]
    #[inline]
    pub const fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.capabilities
    }

    /// Returns limits observed from the opened root authority.
    #[must_use = "inspect the rooted path limits"]
    #[inline]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        self.limits
    }

    /// Validates that a normalized backend path resolves to a directory.
    /// Propagates path/policy and native inspection errors; a resolved
    /// non-directory returns a structured type-conflict error.
    pub(crate) fn validate_directory(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<()> {
        self.validate_directory_until(path, symlink_policy, None)
    }

    /// Validates a directory while enforcing an enclosing operation deadline.
    pub(crate) fn validate_directory_until(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
        deadline: Option<std::time::Instant>,
    ) -> LocalResult<()> {
        validate_rooted_list_start(&self.root, path, symlink_policy, deadline).map(|_| ())
    }

    /// Observes objective path limits through the nearest existing rooted
    /// entry or ancestor.
    /// Propagates path/policy, native open, and capability-probe errors. A
    /// successfully probed but unavailable limit is represented as unknown.
    pub(crate) fn limits_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileSystemLimits> {
        let file = self.open_nearest_probe(path, symlink_policy)?;
        match crate::capability::probe_limits(&file) {
            Ok(limits) => Ok(limits),
            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
        }
    }

    /// Observes dynamic capacity through the nearest existing rooted entry or
    /// ancestor.
    /// Propagates path/policy, native open, and capacity-probe errors. The
    /// returned observation does not reserve space for a later operation.
    pub(crate) fn space_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<crate::LocalFileSystemSpace> {
        let file = self.open_nearest_probe(path, symlink_policy)?;
        match crate::capability::probe_space(&file) {
            Ok(space) => Ok(space),
            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
        }
    }

    /// Opens the nearest existing entry for a capability probe.
    /// Falls back to parents only for `NotFound`. Returns other path/policy or
    /// native errors, including rejection of non-file/non-directory entries.
    fn open_nearest_probe(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<std::fs::File> {
        if path.as_os_str().is_empty() {
            return match self.root.open_probe_root() {
                Ok(file) => Ok(file),
                Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
            };
        }
        let mut candidate =
            resolve_rooted_path_allow_root(&self.root, path, symlink_policy, true, LocalFileOperation::Capabilities)?;
        loop {
            if candidate.as_os_str().is_empty() {
                return match self.root.open_probe_root() {
                    Ok(file) => Ok(file),
                    Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
                };
            }
            let candidate_path = rooted_path(&candidate, LocalFileOperation::Capabilities)?;
            match self.root.open_probe_file(&candidate_path) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if !candidate.pop() {
                        return match self.root.open_probe_root() {
                            Ok(file) => Ok(file),
                            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
                        };
                    }
                }
                Err(error) => {
                    return Err(rooted_io_error(LocalFileOperation::Capabilities, path, error));
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod deleted_cwd_constructor_tests {
    use std::fs;
    use std::path::Path;
    use std::process::Command;

    use super::RootedLocalFileSystem;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::LocalSymlinkPolicy;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::resolution::ResolutionContract;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_deleted_process_cwd_preserves_constructor_and_pair_operation_context() {
        const CHILD: &str = "RS_LOCAL_FILES_DELETED_CWD_CONSTRUCTOR_CHILD";
        const TEST: &str = "rooted_local_file_system::deleted_cwd_constructor_tests::test_deleted_process_cwd_preserves_constructor_and_pair_operation_context";
        if std::env::var_os(CHILD).is_none() {
            let output = Command::new(std::env::current_exe().expect("test executable should be available"))
                .args(["--exact", TEST, "--test-threads=1", "--nocapture"])
                .env(CHILD, "1")
                .output()
                .expect("isolated child test should start");
            assert!(
                output.status.success(),
                "isolated child failed: stdout={} stderr={}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("DELETED_CWD_CONSTRUCTORS_COMPLETE"));
            return;
        }

        let original = std::env::current_dir().expect("child's original cwd should be available");
        let fixture = tempfile::tempdir().expect("disposable fixture should exist");
        let cwd = fixture.path().join("removed-current-directory");
        fs::create_dir(&cwd).expect("disposable cwd should exist");
        let host = LocalFileSystem::host().expect("Host construction should not query cwd");
        std::env::set_current_dir(&cwd).expect("only this child should enter the disposable cwd");
        fs::remove_dir(&cwd).expect("only the empty disposable child cwd should be removed");

        let requested = Path::new(".");
        let root_result = RootedLocalFileSystem::open_with_contract(
            requested,
            ResolutionContract::unrestricted(),
            LocalSymlinkPolicy::FollowWithinScope,
        );
        let directory_result = host.open_directory_with_options(requested, &LocalDirectoryOpenOptions::new());
        let source = Path::new("relative-source");
        let target = Path::new("relative-target");
        let copy_result = host.copy(source, target);
        let rename_result = host.rename(source, target);
        // Restore a live cwd before assertions and automatic fixture cleanup.
        std::env::set_current_dir(&original).expect("child should restore its original cwd");

        let root_error = root_result.expect_err("opening a relative root requires a readable process cwd");
        let directory_error = directory_result.expect_err("opening a relative Host directory requires a readable cwd");
        for error in [&root_error, &directory_error] {
            assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
            assert_eq!(error.path(), Some(requested));
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
            assert!(error.io_error().is_some());
        }

        let copy = copy_result.expect_err("relative copy operands require a process cwd snapshot");
        let rename = rename_result.expect_err("relative rename operands require a process cwd snapshot");
        for (error, operation) in [
            (copy.error(), LocalFileOperation::Copy),
            (rename.error(), LocalFileOperation::Rename),
        ] {
            assert_eq!(error.operation(), operation);
            assert_eq!(error.path(), Some(source));
            assert_eq!(error.target(), Some(target));
            assert_eq!(error.current_directory(), None);
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
        }
        println!("DELETED_CWD_CONSTRUCTORS_COMPLETE");
    }

    #[test]
    fn test_native_root_constructor_cursor_failure_retains_requested_root() {
        let path = Path::new("/");
        // Opening the native root has no relative components, so the first cursor is
        // constructor-owned.
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::OpenCursor, 1);
        let error = RootedLocalFileSystem::open_with_contract(
            path,
            ResolutionContract::unrestricted(),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("the root constructor must propagate cursor creation failure");

        assert!(guard.fired());
        assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
        assert_eq!(error.path(), Some(path));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
        assert_eq!(
            error
                .io_error()
                .expect("native cursor source should survive")
                .to_string(),
            "injected native I/O contract failure"
        );
        drop(guard);
        RootedLocalFileSystem::open(path).expect("dropping the fault guard should restore normal root construction");
    }
}
