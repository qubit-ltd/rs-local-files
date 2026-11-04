// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Persistence operand validation independent of creation-time PWD.

use std::path::Component;
use std::path::Path;

use super::LocalTempResourceBackend;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::path::LocalFileSystemScope;
use crate::path::LocalNamespacePath;
use crate::path::LocalPathResolver;

/// Validates and binds an absolute target or an explicitly based relative
/// target.
///
/// Performs no I/O. Rejects empty/NUL targets, relative or dotted bases,
/// ambiguous prefixes, and Rooted escapes. Native Host parent traversal is
/// preserved. Errors retain the offending base or target as their structured
/// path.
pub(crate) fn prepare_persist_target(
    scope: LocalFileSystemScope,
    base: Option<&Path>,
    target: &Path,
) -> LocalResult<LocalNamespacePath> {
    if target.as_os_str().is_empty() {
        return Err(invalid(target, "persistence target must not be empty"));
    }
    // Resolve first only to validate native encoding; any other binding error
    // is handled below with the explicit target/base contract.
    let anchor = match scope {
        LocalFileSystemScope::Host => LocalPathResolver::absolute_host(),
        LocalFileSystemScope::Rooted => LocalPathResolver::new(scope, Path::new("/"))?,
    };
    if target.as_os_str().as_encoded_bytes().contains(&0) {
        return anchor.resolve(target);
    }
    let resolver = if let Some(base) = base {
        if !namespace_absolute(scope, base) || contains_dot_component(base) {
            return Err(invalid(
                base,
                "persistence base must be absolute without dot or parent components",
            ));
        }
        if target.has_root() || target.components().any(|part| matches!(part, Component::Prefix(_))) {
            return Err(invalid(target, "persist_at requires a strictly relative target"));
        }
        LocalPathResolver::new(scope, base)?
    } else {
        if !namespace_absolute(scope, target) {
            return Err(invalid(
                target,
                "persist requires a namespace-absolute target; use persist_at for relative targets",
            ));
        }
        anchor
    };
    resolver.resolve(target)
}

/// Verifies an explicit base directory through the resource's existing
/// authority.
///
/// Performs read-only native traversal with the captured symlink policy.
/// Returns a contextual native error or `NotDirectory` without closing the
/// source.
pub(crate) fn validate_persist_base(
    backend: &LocalTempResourceBackend,
    base: &Path,
    policy: LocalSymlinkPolicy,
) -> LocalResult<()> {
    let result = match backend {
        LocalTempResourceBackend::Host(_) => {
            let path = crate::local::resolve_host_path(base, policy, true)?;
            std::fs::metadata(path).map(|metadata| metadata.is_dir())
        }
        LocalTempResourceBackend::Rooted(rooted) => {
            let bound = LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/"))?.resolve(base)?;
            let relative = bound.authority_relative();
            if relative.as_os_str().is_empty() {
                rooted
                    .root
                    .metadata()
                    .map(|metadata| metadata.kind() == crate::rooted::EntryKind::Directory)
            } else {
                let path = crate::rooted_local_file_system::resolve_rooted_path_allow_root(
                    &rooted.root,
                    relative,
                    policy,
                    true,
                    LocalFileOperation::PersistTemp,
                )?;
                let metadata = if path.as_os_str().is_empty() {
                    rooted.root.metadata()
                } else {
                    crate::local::LocalRelativePath::new(&path)
                        .and_then(|relative| rooted.root.symlink_metadata(&relative))
                };
                metadata.map(|metadata| metadata.kind() == crate::rooted::EntryKind::Directory)
            }
        }
    };
    let directory = result.map_err(|error| {
        LocalFileError::from_io(LocalFileOperation::PersistTemp, Some(base.to_path_buf()), None, error)
    })?;
    if !directory {
        return Err(
            LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::PersistTemp)
                .with_path(base.to_path_buf())
                .with_reason("persistence base must be an existing directory"),
        );
    }
    Ok(())
}

/// Tests namespace-specific absolute syntax without reading a process PWD.
fn namespace_absolute(scope: LocalFileSystemScope, path: &Path) -> bool {
    match scope {
        LocalFileSystemScope::Host => path.is_absolute(),
        LocalFileSystemScope::Rooted => {
            path.has_root() && !path.components().any(|part| matches!(part, Component::Prefix(_)))
        }
    }
}

/// Detects Unix dots including interior components hidden by Path::components.
#[cfg(unix)]
fn contains_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|part| part == b"." || part == b"..")
}

/// Detects Windows dot components without normalizing their UTF-16 spelling.
#[cfg(windows)]
fn contains_dot_component(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    let units: Vec<_> = path.as_os_str().encode_wide().collect();
    units
        .split(|unit| *unit == u16::from(b'/') || *unit == u16::from(b'\\'))
        .any(|part| part == [u16::from(b'.')] || part == [u16::from(b'.'), u16::from(b'.')])
}

/// Unsupported platforms cannot validate a native persistence base.
#[cfg(not(any(unix, windows)))]
fn contains_dot_component(_path: &Path) -> bool {
    true
}

/// Returns a structured invalid-operand error preserving its original spelling.
fn invalid(path: &Path, reason: &'static str) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::PersistTemp)
        .with_path(path.to_path_buf())
        .with_reason(reason)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::LocalFileSystemScope;
    use super::LocalSymlinkPolicy;
    use super::LocalTempResourceBackend;
    use super::prepare_persist_target;
    use super::validate_persist_base;
    use crate::error::LocalFileErrorKind;
    use crate::temp::internal::HostTempResourceBackend;
    use crate::temp::internal::TempEntryIdentity;

    #[test]
    fn persist_target_validation_covers_bases_and_namespace_rules() {
        let fixture = tempfile::tempdir().expect("fixture");
        let file = fixture.path().join("file");
        std::fs::write(&file, b"base file").expect("file base");

        let backend = LocalTempResourceBackend::Host(HostTempResourceBackend {
            identity: TempEntryIdentity::from_path(fixture.path()).expect("directory identity"),
            sandbox_path: fixture.path().to_path_buf(),
        });
        validate_persist_base(&backend, fixture.path(), LocalSymlinkPolicy::Reject).expect("directory base");

        let error = validate_persist_base(&backend, &file, LocalSymlinkPolicy::Reject)
            .expect_err("regular file is not a persistence base");
        assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
        let missing = fixture.path().join("missing");
        assert!(validate_persist_base(&backend, &missing, LocalSymlinkPolicy::Reject).is_err());

        let target = fixture.path().join("published");
        assert!(prepare_persist_target(LocalFileSystemScope::Host, None, &target).is_ok());
        assert!(prepare_persist_target(LocalFileSystemScope::Host, Some(fixture.path()), Path::new("child")).is_ok());

        let dotted = fixture.path().join("..");
        let error = prepare_persist_target(LocalFileSystemScope::Host, Some(&dotted), Path::new("child"))
            .expect_err("persistence base cannot contain parent components");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
        let error = prepare_persist_target(LocalFileSystemScope::Rooted, None, Path::new("../escape"))
            .expect_err("rooted persistence cannot escape its namespace");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    }
}
