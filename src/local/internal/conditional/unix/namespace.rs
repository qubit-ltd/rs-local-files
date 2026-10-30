// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative Unix namespace authority.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use super::ConditionalEntry;
use super::recovery::EntryAccess;
use crate::local::internal::RootedParentMode;
use crate::local::internal::ensure_parent_path_with_sync_dirs;
use crate::local::internal::open_rooted_parent;
use crate::options::LocalWriteOptions;
use crate::recovery::LocalRecoveryFileRole;

/// Retained authority for conditional operations in one parent directory.
///
/// Operations address entries by single-component names relative to `parent`;
/// stored paths describe or diagnose those entries and are not used as access
/// authority. Cloned parent handles in recovery entries preserve the ability
/// to reopen and verify a name after this namespace is dropped.
#[derive(Debug)]
pub(super) struct Namespace {
    pub(super) parent: File,
    pub(super) target_name: CString,
    pub(super) sync_dirs: Vec<File>,
    pub(super) namespace_path: PathBuf,
    pub(super) diagnostic_path: Option<PathBuf>,
}

impl Namespace {
    pub(super) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        let parent_path = absolute.parent().unwrap_or(Path::new("/"));
        let name = absolute
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no final component"))?;
        let created = if options.creates_parent() {
            ensure_parent_path_with_sync_dirs(&absolute)?
        } else {
            Vec::new()
        };
        let parent = File::open(parent_path)?;
        let mut sync_dirs = Vec::new();
        for directory in created {
            if let Some(parent) = directory.parent() {
                sync_dirs.push(File::open(parent)?);
            }
        }
        Ok(Self {
            parent,
            target_name: CString::new(name.as_bytes()).map_err(invalid_name)?,
            sync_dirs,
            namespace_path: absolute.clone(),
            diagnostic_path: Some(absolute),
        })
    }

    pub(super) fn open_rooted(
        root: File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        let relative = crate::LocalRelativePath::new(relative)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mode = if options.creates_parent() {
            RootedParentMode::CreateMissingAndTrackSync
        } else {
            RootedParentMode::OpenExisting
        };
        let (parent, target_name, sync_dirs) =
            open_rooted_parent(&root, namespace_absolute, &relative, mode)?.into_parts();
        Ok(Self {
            parent,
            target_name,
            sync_dirs,
            namespace_path: namespace_absolute.to_path_buf(),
            diagnostic_path: diagnostic.map(Path::to_path_buf),
        })
    }

    pub(super) fn create_private(&self, prefix: &str) -> io::Result<(CString, File)> {
        let prefix = format!(".qubit-{prefix}-");
        for _ in 0..128 {
            let generated = crate::local::try_random_file_name(".qubit-conditional-", Some(&prefix), None)?;
            let name = CString::new(generated).map_err(invalid_name)?;
            match open_at(
                &self.parent,
                &name,
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            ) {
                Ok(file) => return Ok((name, file)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique conditional staging entry",
        ))
    }

    pub(super) fn entry(
        &self,
        role: LocalRecoveryFileRole,
        name: CString,
        _unused: Option<()>,
    ) -> io::Result<ConditionalEntry> {
        #[cfg(test)]
        super::test_support::fail_if_requested(super::test_support::FailurePoint::EntryIdentity)?;
        let (device, inode) = identity_at(&self.parent, &name)?;
        #[cfg(test)]
        super::test_support::fail_if_requested(super::test_support::FailurePoint::EntryParentClone)?;
        let parent = self.parent.try_clone()?;
        let access = EntryAccess::new(parent, name.clone(), device, inode, self.diagnostic_path.clone());
        let display = self
            .namespace_path
            .parent()
            .unwrap_or(Path::new(""))
            .join(std::ffi::OsStr::from_bytes(name.as_bytes()));
        Ok(ConditionalEntry::new(
            role,
            display,
            self.diagnostic_path.clone(),
            Box::new(access),
        ))
    }

    pub(super) fn open_name(&self, name: &CString) -> io::Result<File> {
        open_regular_at(&self.parent, name)
    }

    pub(super) fn sync_all(&self) -> io::Result<()> {
        self.parent.sync_all()?;
        for directory in self.sync_dirs.iter().rev() {
            directory.sync_all()?;
        }
        Ok(())
    }

    pub(super) fn verify(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.entry_identity(entry).map(|_| ())
    }

    fn entry_identity(&self, entry: &ConditionalEntry) -> io::Result<(u64, u64)> {
        let metadata = entry.open_reader().map_err(io::Error::other)?.as_file().metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    pub(super) fn remove(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.verify(entry)?;
        let name = super::recovery::name(entry)?;
        // SAFETY: the parent descriptor and single-component name remain live
        // for unlinkat; no symlink is followed by unlinkat.
        let result = unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), 0) };
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    pub(super) fn replace_target(&self, bytes: &[u8]) -> io::Result<()> {
        match open_at(
            &self.parent,
            &self.target_name,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        ) {
            Ok(mut file) => file.write_all(bytes),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let (name, mut replacement) = self.create_private("race")?;
                replacement.write_all(bytes)?;
                // SAFETY: both single-component names and the retained parent
                // descriptor remain live for this test-only competing actor.
                let result = unsafe {
                    libc::renameat(
                        self.parent.as_raw_fd(),
                        name.as_ptr(),
                        self.parent.as_raw_fd(),
                        self.target_name.as_ptr(),
                    )
                };
                if result == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
    pub(super) fn overwrite_target(&self, bytes: &[u8]) -> io::Result<()> {
        let mut file = open_at(
            &self.parent,
            &self.target_name,
            libc::O_WRONLY | libc::O_TRUNC | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0,
        )?;
        file.write_all(bytes)
    }
}

#[allow(clippy::unnecessary_cast)] // Native stat fields differ across Unix targets.
pub(super) fn identity_at(parent: &File, name: &CString) -> io::Result<(u64, u64)> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `parent` is an open directory handle, `name` is NUL terminated,
    // and `metadata` points to writable storage. AT_SYMLINK_NOFOLLOW records
    // the named object itself so post-publication receipts can retain a
    // substituted symlink without opening its target.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            metadata.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatat initialized the output structure.
    let metadata = unsafe { metadata.assume_init() };
    Ok((metadata.st_dev as u64, metadata.st_ino as u64))
}

pub(super) fn open_regular_at(parent: &File, name: &CString) -> io::Result<File> {
    let file = open_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        0,
    )?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "conditional entry is not a regular file",
        ));
    }
    Ok(file)
}

fn open_at(parent: &File, name: &CString, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: the directory handle and NUL-terminated component are live for
    // the call. A successful descriptor is immediately wrapped as owned.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, libc::c_uint::from(mode)) };
    if descriptor == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: openat returned a new descriptor not owned elsewhere.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

fn invalid_name(_: std::ffi::NulError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "path component contains NUL")
}

#[cfg(test)]
mod invalid_operand_contract_tests {
    use std::fs::File;
    use std::io;
    use std::path::Path;

    use super::Namespace;
    use crate::options::LocalWriteOptions;

    #[test]
    fn native_namespace_rejects_root_nul_and_invalid_relative_operands() {
        let fixture = tempfile::tempdir().expect("fixture");
        let options = LocalWriteOptions::default();
        let error = Namespace::open_host(Path::new("/"), &options).expect_err("root has no target name");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let error =
            Namespace::open_host(&fixture.path().join("bad\0name"), &options).expect_err("native name contains nul");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        for relative in [Path::new("../escape"), Path::new("/absolute")] {
            let error = Namespace::open_rooted(
                File::open(fixture.path()).expect("real root authority"),
                relative,
                Path::new("/target"),
                None,
                &options,
            )
            .expect_err("relative authority operand must be valid");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        }
        assert_eq!(std::fs::read_dir(fixture.path()).expect("fixture readable").count(), 0);
    }
}
