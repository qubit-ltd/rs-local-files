// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows conditional namespace permissions and entry identity checks.
//!
//! Retained handles and verified file identities grant access authority.
//! Diagnostic paths describe namespace entries but never grant permission to
//! reopen them; identity checks bind reopened entries to their original handle.

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use self::entry_access::EntryAccess;
use self::scope::Scope;
use self::windows_handle::delete_open_entry;
use self::windows_handle::open_host_directory_no_reparse;
use self::windows_handle::open_host_file;
use self::windows_handle::verify_directory_no_reparse;
use self::windows_handle::verify_regular_no_reparse;
use super::super::ConditionalEntry;
use crate::LocalRelativePath;
use crate::local::internal::windows_rooted::create_rooted_directory;
use crate::local::internal::windows_rooted::native::nt_open_at_with_share_mode;
use crate::local::internal::windows_rooted::open_conditional_entry_at;
use crate::local::internal::windows_rooted::open_conditional_parent;
use crate::local::internal::windows_rooted::open_root_directory;
use crate::local::internal::windows_rooted::rename_conditional_entry_at;
use crate::options::LocalWriteOptions;
use crate::recovery::LocalRecoveryFileRole;
mod entry_access;
pub(super) mod file_identity;
mod scope;
mod windows_handle;
pub(super) use self::file_identity::FileIdentity;
pub(super) use self::windows_handle::file_identity;

pub(super) const ENTRY_ACCESS: u32 = GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

/// Holds the directory handle and identity context for conditional entries.
#[derive(Debug)]
pub(super) struct Namespace {
    /// Selects the host-path or rooted access strategy.
    scope: Scope,
    /// Retained handle for the target's containing directory.
    parent: File,
    /// Leaf name of the target entry.
    target_name: OsString,
    /// Namespace path used to identify the target for internal operations.
    target_path: PathBuf,
    /// Optional user-facing path used only for diagnostics.
    diagnostic_path: Option<PathBuf>,
    /// Retained directory handles for recovery containers.
    recovery_parents: RefCell<Vec<(PathBuf, File)>>,
    /// Test-only switch for exercising recovery-container verification failure.
    #[cfg(test)]
    fail_container_verification: Cell<bool>,
}

impl Namespace {
    /// Opens the host parent directory and records the target leaf name.
    ///
    /// # Parameters
    ///
    /// - `path`: Target path whose parent is retained.
    /// - `options`: Write options controlling parent creation.
    ///
    /// # Returns
    ///
    /// The namespace retaining the target parent handle and leaf name.
    ///
    /// # Errors
    ///
    /// Returns an error if parent creation or directory opening fails, or if
    /// `path` has no filename.
    pub(super) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        let parent_path = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        if options.creates_parent() {
            fs::create_dir_all(&parent_path)?;
        }
        let target_name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "conditional target has no filename"))?
            .to_os_string();
        let parent = open_root_directory(&parent_path)?;
        Ok(Self {
            scope: Scope::Host {
                parent_path: parent_path.clone(),
            },
            parent,
            target_name,
            target_path: path.to_path_buf(),
            diagnostic_path: Some(path.to_path_buf()),
            recovery_parents: RefCell::new(Vec::new()),
            #[cfg(test)]
            fail_container_verification: Cell::new(false),
        })
    }

    /// Opens the target parent relative to a retained root directory handle.
    ///
    /// # Parameters
    ///
    /// - `root`: Open root directory handle.
    /// - `relative`: Target path relative to the root.
    /// - `namespace_absolute`: Absolute namespace path used for internal
    ///   identity.
    /// - `diagnostic`: Optional user-facing path for errors.
    /// - `options`: Write options controlling parent creation.
    ///
    /// # Returns
    ///
    /// The namespace retaining the rooted parent handle and target leaf name.
    ///
    /// # Errors
    ///
    /// Returns an error if `relative` is invalid or a rooted directory
    /// operation fails.
    pub(super) fn open_rooted(
        root: File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        let relative = LocalRelativePath::new(relative)?;
        if options.creates_parent()
            && let Some(parent) = relative.as_path().parent().filter(|path| !path.as_os_str().is_empty())
        {
            let parent = LocalRelativePath::new(parent)?;
            create_rooted_directory(&root, Path::new(""), &parent, true, true)?;
        }
        let (parent, target_name) = open_conditional_parent(&root, &relative)?;
        Ok(Self {
            scope: Scope::Rooted {
                namespace_absolute: namespace_absolute.to_path_buf(),
            },
            parent,
            target_path: namespace_absolute.to_path_buf(),
            diagnostic_path: diagnostic.map(Path::to_path_buf),
            target_name,
            recovery_parents: RefCell::new(Vec::new()),
            #[cfg(test)]
            fail_container_verification: Cell::new(false),
        })
    }
    /// Returns the target leaf name used for handle-relative operations.
    ///
    /// # Returns
    ///
    /// Borrows the leaf name retained when the namespace was opened.
    #[must_use]
    #[inline]
    pub(super) fn target_name(&self) -> &OsStr {
        &self.target_name
    }
    /// Returns the namespace path associated with the target entry.
    ///
    /// # Returns
    ///
    /// Borrows the namespace path retained for this target.
    #[must_use]
    #[inline]
    pub(super) fn target_path(&self) -> &Path {
        &self.target_path
    }
    /// Reports whether this namespace is anchored to a retained root handle.
    ///
    /// # Returns
    ///
    /// `true` for rooted access and `false` for host-path access.
    #[must_use]
    #[inline]
    pub(super) fn is_rooted(&self) -> bool {
        matches!(self.scope, Scope::Rooted { .. })
    }

    /// Returns the host parent path, or an unsupported error for rooted access.
    ///
    ///
    /// # Returns
    ///
    /// Borrows the host parent path when host-path access is active.
    /// # Errors
    ///
    /// Returns `Unsupported` when this namespace uses rooted access.
    pub(super) fn host_parent_path(&self) -> io::Result<&Path> {
        match &self.scope {
            Scope::Host { parent_path } => Ok(parent_path),
            Scope::Rooted { .. } => Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted publication has no Host path",
            )),
        }
    }
    /// Builds the host path for a private entry; rooted namespaces reject it.
    ///
    /// # Parameters
    ///
    /// - `name`: Private entry leaf name.
    ///
    /// # Returns
    ///
    /// The host path for `name` when host-path access is active.
    /// # Errors
    ///
    /// Returns `Unsupported` when this namespace uses rooted access.
    #[inline]
    pub(super) fn private_file_path(&self, name: &OsStr) -> io::Result<PathBuf> {
        self.host_child_path(name)
    }

    /// Creates a uniquely named recovery directory and backup entry path.
    ///
    ///
    /// # Returns
    ///
    /// The recovery directory path and its backup entry path.
    /// # Errors
    ///
    /// Returns an error if a unique recovery directory cannot be created or
    /// verified.
    pub(super) fn backup_container(&self) -> io::Result<(PathBuf, PathBuf)> {
        self.backup_container_with_names(|| {
            let dirname = crate::local::internal::file_name_generation::try_random_file_name(
                ".qubit-conditional-recovery-",
                None,
                None,
            )?;
            let backup_name =
                crate::local::internal::file_name_generation::try_random_file_name("displaced-", None, Some(".bak"))?;
            Ok((dirname.into(), backup_name.into()))
        })
    }

    /// Creates and retains a verified recovery directory using supplied names.
    ///
    /// # Type Parameters
    ///
    /// - `F`: Callback type used to generate the directory and backup names.
    ///
    /// # Parameters
    ///
    /// - `next`: Callback that supplies a container and backup leaf name.
    ///
    ///
    /// # Returns
    ///
    /// The created recovery directory path and its backup entry path.
    /// # Errors
    ///
    /// Returns an error if host access, name generation, directory creation, or
    /// identity verification fails.
    pub(super) fn backup_container_with_names<F>(&self, mut next: F) -> io::Result<(PathBuf, PathBuf)>
    where
        F: FnMut() -> io::Result<(OsString, OsString)>,
    {
        let parent = self.host_parent_path()?;
        for _ in 0..32 {
            let (dirname, backup_name) = next()?;
            let container = parent.join(&dirname);
            let handle = nt_open_at_with_share_mode(
                &self.parent,
                OsStr::new(&dirname),
                windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
            );
            match handle {
                Ok(handle) => {
                    if let Err(primary) = self.verify_new_container(&handle) {
                        let cleanup = delete_open_entry(&handle);
                        return Err(match cleanup {
                            Ok(()) => primary,
                            Err(cleanup) => io::Error::new(
                                primary.kind(),
                                format!(
                                    "{}; bound recovery container cleanup failed at {}: {}",
                                    primary,
                                    container.display(),
                                    cleanup
                                ),
                            ),
                        });
                    }
                    self.recovery_parents.borrow_mut().push((container.clone(), handle));
                    return Ok((container.clone(), container.join(backup_name)));
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!(
                            "failed to create and bind recovery container {}: {}",
                            container.display(),
                            error
                        ),
                    ));
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "could not allocate a unique recovery container",
        ))
    }

    /// Injects one verification failure for recovery cleanup tests.
    #[cfg(test)]
    pub(super) fn fail_next_container_verification(&self) {
        self.fail_container_verification.set(true);
    }
    /// Compares a recovery path's current identity with its retained handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Recovery container path whose identity is checked.
    ///
    /// # Returns
    ///
    /// `true` when `path` resolves to the retained directory; `false` when no
    /// handle is retained for it. # Errors
    ///
    /// Returns an error if the path cannot be opened or either file identity
    /// cannot be read.
    #[cfg(test)]
    pub(super) fn recovery_container_is_bound(&self, path: &Path) -> io::Result<bool> {
        let parents = self.recovery_parents.borrow();
        let Some((_, handle)) = parents.iter().find(|(container, _)| container == path) else {
            return Ok(false);
        };
        let current = open_host_directory_no_reparse(path)?;
        Ok(file_identity(handle)? == file_identity(&current)?)
    }

    /// Creates an entry permission bound to a recovery container handle.
    ///
    /// # Parameters
    ///
    /// - `role`: Recovery role assigned to the entry.
    /// - `container`: Recovery directory containing the entry.
    /// - `backup_path`: Backup path whose leaf is opened.
    ///
    ///
    /// # Returns
    ///
    /// A conditional entry bound to the backup file's observed identity.
    /// # Errors
    ///
    /// Returns an error if the backup path has no leaf or its directory, file,
    /// or identity cannot be opened.
    pub(super) fn entry_in_container(
        &self,
        role: LocalRecoveryFileRole,
        container: &Path,
        backup_path: &Path,
    ) -> io::Result<ConditionalEntry> {
        let name = backup_path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "backup path has no leaf"))?
            .to_os_string();
        let parent = open_host_directory_no_reparse(container)?;
        let file = open_conditional_entry_at(&parent, &name, ENTRY_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?;
        let identity = file_identity(&file)?;
        let diagnostic_path = self
            .recovery_parents
            .borrow()
            .iter()
            .find(|(known_container, _)| known_container == container)
            .map(|(known_container, _)| known_container.join(&name));
        self.recovery_parents
            .borrow_mut()
            .push((container.to_path_buf(), parent.try_clone()?));
        let access = EntryAccess {
            parent,
            name: name.clone(),
            diagnostic_path: diagnostic_path.clone(),
            identity,
            _retained: None,
        };
        Ok(ConditionalEntry::new(
            role,
            backup_path.to_path_buf(),
            diagnostic_path,
            Box::new(access),
        ))
    }

    /// Creates an entry permission tied to an already-open file identity.
    ///
    /// # Parameters
    ///
    /// - `role`: Recovery role assigned to the entry.
    /// - `name`: Entry leaf name.
    /// - `file`: Open handle whose identity is retained.
    ///
    ///
    /// # Returns
    ///
    /// A conditional entry bound to the supplied file identity and retained
    /// handle. # Errors
    ///
    /// Returns an error if the file is not regular, its identity cannot be
    /// read, or handles cannot be cloned.
    pub(super) fn entry_bound_to(
        &self,
        role: LocalRecoveryFileRole,
        name: OsString,
        file: &File,
    ) -> io::Result<ConditionalEntry> {
        let identity = file_identity(file)?;
        verify_regular_no_reparse(file)?;
        let diagnostic_path = self
            .diagnostic_path
            .as_deref()
            .and_then(|path| path.parent().map(|parent| parent.join(&name)));
        let access = EntryAccess {
            parent: self.parent.try_clone()?,
            name: name.clone(),
            diagnostic_path: diagnostic_path.clone(),
            identity,
            _retained: Some(file.try_clone()?),
        };
        Ok(ConditionalEntry::new(
            role,
            self.child_path(&name),
            diagnostic_path,
            Box::new(access),
        ))
    }

    /// Deletes a retained recovery directory after verifying its handle.
    ///
    /// # Parameters
    ///
    /// - `container`: Recovery directory to remove after handle verification.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` for rooted access and an error if the retained
    /// directory cannot be verified or deleted.
    pub(super) fn remove_container(&self, container: &Path) -> io::Result<()> {
        if self.is_rooted() {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted publication has no Host recovery container",
            ));
        }
        let mut recovery_parents = self.recovery_parents.borrow_mut();
        let index = recovery_parents
            .iter()
            .position(|(path, _)| path == container)
            .ok_or_else(|| {
                io::Error::new(
                    ErrorKind::NotFound,
                    "conditional recovery container handle is unavailable",
                )
            })?;
        let directory = &recovery_parents[index].1;
        verify_directory_no_reparse(directory)?;
        delete_open_entry(directory)?;
        recovery_parents.remove(index);
        Ok(())
    }

    /// Joins a leaf name to this namespace's diagnostic location.
    ///
    /// # Parameters
    ///
    /// - `name`: Entry leaf name to append.
    ///
    /// # Returns
    ///
    /// The diagnostic child path formed from this namespace and `name`.
    #[must_use]
    pub(super) fn child_path(&self, name: &OsStr) -> PathBuf {
        match &self.scope {
            Scope::Host { parent_path } => parent_path.join(name),
            Scope::Rooted { namespace_absolute, .. } => {
                namespace_absolute.parent().unwrap_or_else(|| Path::new("/")).join(name)
            }
        }
    }

    /// Builds a host child path, rejecting rooted namespaces without host
    /// access.
    ///
    /// # Parameters
    ///
    /// - `name`: Entry leaf name to append.
    ///
    ///
    /// # Returns
    ///
    /// The host child path formed from the retained parent and `name`.
    /// # Errors
    ///
    /// Returns `Unsupported` when this namespace uses rooted access.
    pub(super) fn host_child_path(&self, name: &OsStr) -> io::Result<PathBuf> {
        match &self.scope {
            Scope::Host { parent_path } => Ok(parent_path.join(name)),
            Scope::Rooted { .. } => Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted paths cannot use Host namespace operations",
            )),
        }
    }

    /// Opens and records an entry after capturing its identity and file type.
    ///
    /// # Parameters
    ///
    /// - `role`: Recovery role assigned to the entry.
    /// - `name`: Entry leaf name to open and bind.
    ///
    ///
    /// # Returns
    ///
    /// A conditional entry bound to the opened file identity.
    /// # Errors
    ///
    /// Returns an error if the entry cannot be opened, validated, or bound to a
    /// cloned parent handle.
    pub(super) fn entry(&self, role: LocalRecoveryFileRole, name: OsString) -> io::Result<ConditionalEntry> {
        let file = self.open_entry(&name)?;
        let identity = file_identity(&file)?;
        verify_regular_no_reparse(&file)?;
        let diagnostic_path = self
            .diagnostic_path
            .as_deref()
            .and_then(|path| path.parent().map(|parent| parent.join(&name)));
        let access = EntryAccess {
            parent: self.parent.try_clone()?,
            name: name.clone(),
            diagnostic_path: diagnostic_path.clone(),
            identity,
            _retained: None,
        };
        Ok(ConditionalEntry::new(
            role,
            self.child_path(&name),
            diagnostic_path,
            Box::new(access),
        ))
    }

    /// Creates a private entry beneath the retained parent, cleaning failures
    /// safely.
    ///
    /// # Parameters
    ///
    /// - `prefix`: Name prefix identifying the private entry purpose.
    ///
    ///
    /// # Returns
    ///
    /// The allocated entry leaf name and its open file handle.
    /// # Errors
    ///
    /// Returns an error if allocation fails or cleanup cannot safely remove a
    /// partially created entry.
    pub(super) fn create_private(&self, prefix: &str) -> io::Result<(OsString, File)> {
        for _ in 0..32 {
            let name = OsString::from(crate::local::internal::file_name_generation::try_random_file_name(
                &format!(".qubit-{prefix}-"),
                None,
                None,
            )?);
            match crate::temp::internal::privacy_windows::create_private_file_at(&self.parent, &name) {
                Ok(file) => return Ok((name, file)),
                Err(error) => {
                    let crate::temp::types::NativePrivateCreateError { source, retained } = error;
                    if let Some(mut retained) = retained {
                        let path = self.child_path(&retained.name);
                        let cleanup = match retained.file.take() {
                            Some(file) => self.delete_created_private(&file, None),
                            None => Err(io::Error::other(
                                "private creation retained no handle that can safely identify the entry",
                            )),
                        };
                        return Err(match cleanup {
                            Ok(()) => source,
                            Err(cleanup) => io::Error::new(
                                source.kind(),
                                super::private_entry_setup_error::PrivateEntrySetupError {
                                    primary: source,
                                    cleanup,
                                    path,
                                },
                            ),
                        });
                    }
                    if source.kind() == ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(source);
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "could not allocate a unique conditional entry",
        ))
    }

    /// Opens a non-directory entry relative to the namespace authority.
    ///
    /// # Parameters
    ///
    /// - `name`: Entry leaf name to open.
    ///
    ///
    /// # Returns
    ///
    /// An open handle to the non-directory entry.
    /// # Errors
    ///
    /// Returns an error if the entry cannot be opened beneath this namespace
    /// authority.
    pub(super) fn open_entry(&self, name: &OsStr) -> io::Result<File> {
        match &self.scope {
            Scope::Host { .. } => open_host_file(&self.host_child_path(name)?, ENTRY_ACCESS),
            Scope::Rooted { .. } => {
                open_conditional_entry_at(&self.parent, name, ENTRY_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE)
            }
        }
    }

    /// Returns an entry identity, using `None` only when the name is absent.
    ///
    /// # Parameters
    ///
    /// - `name`: Entry leaf name whose current identity is queried.
    ///
    ///
    /// # Returns
    ///
    /// `Some(identity)` when the entry exists and `None` only when its name is
    /// absent. # Errors
    ///
    /// Returns an error if opening the entry or reading its identity fails for
    /// a reason other than absence.
    pub(super) fn identity_at(&self, name: &OsStr) -> io::Result<Option<FileIdentity>> {
        match self.open_entry(name) {
            Ok(file) => Ok(Some(file_identity(&file)?)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Reopens an entry and checks that it still denotes the retained object.
    ///
    /// # Parameters
    ///
    /// - `entry`: Permission whose namespace entry is checked.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry cannot be reopened or its current identity
    /// differs from the retained identity.
    pub(super) fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        let retained = entry.open_reader().map_err(io::Error::other)?;
        let path = entry.namespace_path();
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
        let current = if self.is_rooted() || path.parent() == Some(self.host_parent_path()?) {
            self.entry(entry.role(), name.to_os_string())?
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no parent"))?;
            self.entry_in_container(entry.role(), parent, path)?
        };
        let current = current.open_reader().map_err(io::Error::other)?;
        if file_identity(retained.as_file())? != file_identity(current.as_file())? {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "conditional entry name changed identity",
            ));
        }
        Ok(())
    }

    /// Renames entries relative to the retained parent handle.
    ///
    /// # Parameters
    ///
    /// - `source`: Source entry leaf name.
    /// - `destination`: Destination entry leaf name.
    /// - `overwrite`: Whether an existing destination may be replaced.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle-relative rename cannot be completed.
    pub(super) fn rename(&self, source: &OsStr, destination: &OsStr, overwrite: bool) -> io::Result<()> {
        rename_conditional_entry_at(&self.parent, source, &self.parent, destination, overwrite)
    }

    /// Removes an entry only after matching its current identity to its
    /// permission.
    ///
    /// # Parameters
    ///
    /// - `entry`: Permission for the entry to remove.
    ///
    /// # Errors
    ///
    /// Returns an error if the entry identity changed or handle-relative
    /// deletion fails.
    pub(super) fn remove(&self, entry: &ConditionalEntry) -> io::Result<()> {
        let verified = entry.open_reader().map_err(io::Error::other)?;
        let expected_identity = file_identity(verified.as_file())?;
        match &self.scope {
            Scope::Host { .. } => {
                let parent = self.parent_handle_for(entry.namespace_path())?;
                let name = entry
                    .namespace_path()
                    .file_name()
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
                let file = open_conditional_entry_at(
                    &parent,
                    name,
                    windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_OPEN,
                    FILE_NON_DIRECTORY_FILE,
                )?;
                if file_identity(&file)? != expected_identity {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "conditional entry identity changed before removal",
                    ));
                }
                delete_open_entry(&file)
            }
            Scope::Rooted { .. } => {
                let parent = self.parent.try_clone()?;
                let name = entry
                    .namespace_path()
                    .file_name()
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
                let file = open_conditional_entry_at(
                    &parent,
                    name,
                    windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_OPEN,
                    FILE_NON_DIRECTORY_FILE,
                )?;
                if file_identity(&file)? != expected_identity {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "conditional entry identity changed before removal",
                    ));
                }
                delete_open_entry(&file)
            }
        }
    }

    /// Deletes a private file handle after optional identity and type
    /// verification.
    ///
    /// # Parameters
    ///
    /// - `file`: Open private file handle to delete.
    /// - `expected`: Optional identity that the handle must match.
    ///
    /// # Errors
    ///
    /// Returns an error if the expected identity differs, the file is not
    /// regular, or deletion fails.
    pub(super) fn delete_created_private(&self, file: &File, expected: Option<FileIdentity>) -> io::Result<()> {
        if let Some(expected) = expected
            && file_identity(file)? != expected
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "private entry handle identity changed before cleanup",
            ));
        }
        verify_regular_no_reparse(file)?;
        delete_open_entry(file)
    }

    /// Confirms a newly created recovery handle names a real directory.
    ///
    /// # Parameters
    ///
    /// - `handle`: Newly created directory handle to validate.
    ///
    /// # Errors
    ///
    /// Returns an error if the handle does not identify a real directory.
    fn verify_new_container(&self, handle: &File) -> io::Result<()> {
        #[cfg(test)]
        if self.fail_container_verification.replace(false) {
            return Err(io::Error::other("injected recovery container verification failure"));
        }
        verify_directory_no_reparse(handle)
    }

    /// Clones the retained parent handle corresponding to a host entry path.
    ///
    /// # Parameters
    ///
    /// - `path`: Entry path whose retained parent handle is requested.
    ///
    ///
    /// # Returns
    ///
    /// A clone of the retained parent directory handle for `path`.
    /// # Errors
    ///
    /// Returns `InvalidInput` when `path` has no parent and `NotFound` when no
    /// retained parent matches.
    fn parent_handle_for(&self, path: &Path) -> io::Result<File> {
        let parent_path = path
            .parent()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry path has no parent"))?;
        if matches!(&self.scope, Scope::Host { parent_path: expected } if expected == parent_path) {
            return self.parent.try_clone();
        }
        self.recovery_parents
            .borrow()
            .iter()
            .find(|(container, _)| container == parent_path)
            .map(|(_, parent)| parent.try_clone())
            .transpose()?
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "conditional recovery parent handle is unavailable"))
    }
}
