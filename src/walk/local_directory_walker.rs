// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use qubit_budget::ManagedResourcePermit;
use qubit_budget::ManagedResourcePool;
use qubit_budget::ResourceBudget;

use super::internal::RootedWalkFrame;
use super::internal::RootedWalkState;
use super::internal::WalkFrame;
use super::local_directory_walker_support::close_host_frame;
use super::local_directory_walker_support::directory_limit_error;
use super::local_directory_walker_support::directory_pool;
use super::local_directory_walker_support::is_terminal_walk_error;
use super::local_directory_walker_support::name_bytes;
use super::local_directory_walker_support::validate_options;
use super::local_directory_walker_support::walker_deadline;
use crate::LocalDirectoryEntry;
use crate::LocalDirectoryReopenPolicy;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalListOptions;
use crate::LocalResourceKind;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWalkErrorPolicy;
use crate::local::DirectoryIdentity;

/// Lazy depth-first iterator over native local directory entries.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> qubit_local_files::LocalResult<()> {
/// let filesystem = LocalFileSystem::host()?;
/// let walker = filesystem.list(Path::new("."))?;
/// for entry in walker {
///     let entry = entry?;
///     println!("{}", entry.path().display());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct LocalDirectoryWalker {
    /// Namespace-absolute traversal root exposed publicly.
    root: PathBuf,
    /// Bound native traversal root used for filesystem operations.
    backend_root: PathBuf,
    /// Filesystem PWD snapshot retained for errors yielded during iteration.
    current_directory: Option<PathBuf>,
    /// Policy fixed when the walker is created.
    options: LocalListOptions,
    /// Open directory iterators, bounded by traversal depth.
    stack: Vec<WalkFrame>,
    /// Current number of open native directory readers.
    open_directories: Option<ManagedResourcePool<LocalResourceKind, usize>>,
    /// Native directory identities on the active DFS path.
    followed_directories: HashSet<DirectoryIdentity>,
    /// Descriptor-relative traversal state for a rooted walker.
    rooted: Option<RootedWalkState>,
    /// Whether fail-fast error policy has terminated iteration.
    terminated: bool,
    /// Symbolic-link policy fixed when the walker is created.
    symlink_policy: LocalSymlinkPolicy,
    /// Optional budget tracking yielded entries.
    entry_budget: Option<ResourceBudget<LocalResourceKind, usize>>,
    /// Optional budget tracking memory used by duplicate-name detection.
    seen_name_budget: Option<ResourceBudget<LocalResourceKind, usize>>,
    /// Monotonic deadline fixed when the walker is created.
    deadline: Option<Instant>,
}

impl LocalDirectoryWalker {
    /// Opens a Host walker with separate backend and diagnostic roots.
    pub(crate) fn open_with_diagnostic(
        backend_root: PathBuf,
        diagnostic_root: PathBuf,
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        validate_options(&diagnostic_root, &options)?;
        let deadline = walker_deadline(&diagnostic_root, &options)?;
        let metadata = match fs::symlink_metadata(&backend_root) {
            Ok(metadata) => metadata,
            Err(error) => return Err(walk_io_error(&diagnostic_root, error)),
        };
        if !metadata.file_type().is_dir() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::List)
                    .with_path(diagnostic_root.clone()),
            );
        }
        let open_directories = directory_pool(&options);
        let directory_permit = open_directories.as_ref().map(|pool| {
            pool.try_acquire(1)
                .expect("validated non-zero directory capacity accepts root")
        });
        let entries = match fs::read_dir(&backend_root) {
            Ok(entries) => entries,
            Err(error) => return Err(walk_io_error(&diagnostic_root, error)),
        };
        #[cfg(feature = "test-support")]
        if crate::local::test_support_enabled("walker-root-canonicalize") {
            return Err(walk_io_error(
                &diagnostic_root,
                std::io::Error::other("injected walker root canonicalization failure"),
            ));
        }
        let root_identity = native_directory_identity(&metadata, &backend_root)?;
        let mut followed_directories = HashSet::new();
        followed_directories.insert(root_identity.clone());
        Ok(Self {
            root: diagnostic_root,
            backend_root,
            current_directory: None,
            options,
            stack: vec![WalkFrame {
                entries: Some(entries),
                directory_permit,
                seen: HashSet::new(),
                relative: PathBuf::new(),
                identity: Some(root_identity),
                entry_depth: 1,
            }],
            open_directories,
            followed_directories,
            rooted: None,
            terminated: false,
            symlink_policy,
            entry_budget: options
                .max_entries()
                .map(|limit| ResourceBudget::new(LocalResourceKind::Entry, limit)),
            seen_name_budget: options
                .max_seen_name_bytes()
                .map(|limit| ResourceBudget::new(LocalResourceKind::SeenNameBytes, limit)),
            deadline,
        })
    }

    /// Creates a walker derived from an opened rooted authority.
    ///
    /// # Parameters
    ///
    /// - `root`: Shared opened descriptor or handle authority.
    /// - `path`: Optional validated descendant; `None` selects the authority
    ///   root.
    /// - `options`: Traversal policy fixed for the walker lifetime.
    ///
    /// # Returns
    ///
    /// A walker that derives every descendant operation from the opened root.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the rooted start path is invalid. Rooted
    /// directory opening and enumeration errors are yielded by the iterator.
    pub(crate) fn open_rooted(
        root: Arc<crate::rooted::Root>,
        path: Option<crate::local::LocalRelativePath>,
        namespace_root: PathBuf,
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        Self::open_rooted_with_output(root, path, PathBuf::new(), namespace_root, options, symlink_policy)
    }

    /// Creates a rooted walker with separate authority and logical output
    /// paths, which preserves a symlink component in returned paths.
    pub(crate) fn open_rooted_with_output(
        root: Arc<crate::rooted::Root>,
        path: Option<crate::local::LocalRelativePath>,
        output_parent: PathBuf,
        namespace_root: PathBuf,
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let diagnostic_root = root.path().join(match path.as_ref() {
            Some(path) => path.as_path().to_path_buf(),
            None => PathBuf::new(),
        });
        validate_options(&diagnostic_root, &options)?;
        let deadline = walker_deadline(&diagnostic_root, &options)?;
        let authority_parent = match path.as_ref() {
            Some(path) => path.as_path().to_path_buf(),
            None => PathBuf::new(),
        };
        let start_metadata = match path.as_ref() {
            Some(path) => root.symlink_metadata(path),
            None => root.metadata(),
        };
        let start_metadata = match start_metadata {
            Ok(metadata) => metadata,
            Err(error) => return Err(walk_io_error(&namespace_root, error)),
        };
        let start_identity = DirectoryIdentity::from_rooted_metadata(&start_metadata, &authority_parent);
        let mut followed_directories = HashSet::new();
        followed_directories.insert(start_identity.clone());
        Ok(Self {
            root: namespace_root,
            backend_root: PathBuf::new(),
            current_directory: None,
            options,
            stack: Vec::new(),
            open_directories: directory_pool(&options),
            followed_directories: HashSet::new(),
            rooted: Some(RootedWalkState {
                root,
                stack: vec![RootedWalkFrame {
                    reader: None,
                    directory_permit: None,
                    seen: HashSet::new(),
                    authority_parent,
                    output_parent,
                    entry_depth: 1,
                    identity: Some(start_identity),
                }],
                followed_directories,
                symlink_policy,
            }),
            terminated: false,
            symlink_policy,
            entry_budget: options
                .max_entries()
                .map(|limit| ResourceBudget::new(LocalResourceKind::Entry, limit)),
            seen_name_budget: options
                .max_seen_name_bytes()
                .map(|limit| ResourceBudget::new(LocalResourceKind::SeenNameBytes, limit)),
            deadline,
        })
    }

    /// Returns the bound traversal root.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Binds the filesystem PWD snapshot used by yielded errors.
    #[must_use]
    pub(crate) fn bind_current_directory(mut self, current_directory: Option<PathBuf>) -> Self {
        self.current_directory = current_directory;
        self
    }

    /// Determines whether a directory may be opened at the current depth.
    ///
    /// # Parameters
    ///
    /// - `entry_depth`: Depth of the directory entry being yielded.
    ///
    /// # Returns
    ///
    /// `true` when recursion and the configured depth limit permit descent.
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    fn may_descend(&self, entry_depth: usize) -> bool {
        self.options.recursive() && self.options.max_depth().is_none_or(|max_depth| entry_depth < max_depth)
    }

    /// Closes every currently open host reader while retaining its frame.
    ///
    /// Each closed reader drops exactly one acquired directory permit. The
    /// retained frames can later be reopened without changing DFS state.
    fn close_all_host_frames(&mut self) {
        for frame in &mut self.stack {
            close_host_frame(frame);
        }
    }

    /// Pops one host frame after closing its reader, if present.
    ///
    /// # Returns
    ///
    /// Returns the removed frame, or `None` when traversal has no host frame.
    fn pop_host_frame(&mut self) -> Option<WalkFrame> {
        let mut frame = self.stack.pop()?;
        close_host_frame(&mut frame);
        Some(frame)
    }

    /// Acquires capacity for one host directory according to reopen policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Directory path attached to a capacity error.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileErrorKind::ResourceLimit`] when the pool is
    /// exhausted and the configured policy is
    /// [`LocalDirectoryReopenPolicy::Fail`]. Under `Reopen`, all active
    /// readers are closed before retrying the acquisition.
    ///
    /// # Returns
    ///
    /// A permit that remains owned for the opened reader's lifetime, or
    /// `None` when the caller configured no open-directory limit.
    fn acquire_host_directory(
        &mut self,
        path: &Path,
    ) -> LocalResult<Option<ManagedResourcePermit<LocalResourceKind, usize>>> {
        let Some(pool) = self.open_directories.as_ref() else {
            return Ok(None);
        };
        match pool.try_acquire(1) {
            Ok(permit) => Ok(Some(permit)),
            Err(_error) if self.options.reopen_policy() == LocalDirectoryReopenPolicy::Reopen => {
                self.close_all_host_frames();
                match self
                    .open_directories
                    .as_ref()
                    .expect("configured directory budget remains present")
                    .try_acquire(1)
                {
                    Ok(permit) => Ok(Some(permit)),
                    Err(error) => Err(directory_limit_error(path, error)),
                }
            }
            Err(error) => Err(directory_limit_error(path, error)),
        }
    }

    /// Opens a child directory and pushes it onto the traversal stack.
    ///
    /// # Parameters
    ///
    /// - `path`: Bound child path.
    /// - `relative`: Root-relative child path.
    /// - `entry_depth`: Depth of the child directory entry.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for cycles or native directory-open failures.
    fn descend(
        &mut self,
        path: &Path,
        metadata: &fs::Metadata,
        relative: PathBuf,
        entry_depth: usize,
    ) -> LocalResult<()> {
        #[cfg(feature = "test-support")]
        if crate::local::test_support_enabled("walker-descend-canonicalize") {
            return Err(walk_io_error(
                path,
                std::io::Error::other("injected walker descent canonicalization failure"),
            ));
        }
        let identity = native_directory_identity(metadata, path)?;
        if self.followed_directories.contains(&identity) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::List)
                    .with_reason("directory identity cycle detected")
                    .with_path(path.to_path_buf()),
            );
        }
        let directory_permit = self.acquire_host_directory(path)?;
        let entries = fs::read_dir(path).map_err(|error| walk_io_error(path, error))?;
        self.followed_directories.insert(identity.clone());
        self.stack.push(WalkFrame {
            entries: Some(entries),
            directory_permit,
            seen: HashSet::new(),
            relative,
            identity: Some(identity),
            entry_depth: entry_depth + 1,
        });
        Ok(())
    }

    /// Reopens a host frame after its directory reader was released.
    ///
    /// # Parameters
    ///
    /// - `relative_parent`: Root-relative path of the frame to reopen.
    ///
    /// # Errors
    ///
    /// Returns a listing error when the directory cannot be reopened or its
    /// identity changed. The failed frame is discarded in continue mode so
    /// iteration cannot yield the same reopen error forever.
    fn reopen_host_frame(&mut self, relative_parent: &Path) -> LocalResult<()> {
        let directory = self.backend_root.join(relative_parent);
        let diagnostic_directory = self.root.join(relative_parent);
        #[cfg(feature = "test-support")]
        if crate::local::test_support_enabled("walker-reopen-canonicalize") {
            return self.handle_reopen_error(walk_io_error(
                &diagnostic_directory,
                std::io::Error::other("injected walker reopen canonicalization failure"),
            ));
        }
        let metadata = if self.symlink_policy.follows() {
            fs::metadata(&directory)
        } else {
            fs::symlink_metadata(&directory)
        };
        let metadata = match metadata {
            Ok(metadata) if metadata.file_type().is_dir() => metadata,
            Ok(_) => {
                return self.handle_reopen_error(
                    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::List)
                        .with_reason("directory entry changed while reopening walker frame")
                        .with_path(diagnostic_directory),
                );
            }
            Err(error) => {
                return self.handle_reopen_error(walk_io_error(&diagnostic_directory, error));
            }
        };
        let identity = match native_directory_identity(&metadata, &directory) {
            Ok(identity) => identity,
            Err(error) => return self.handle_reopen_error(error),
        };
        if self.stack.last().and_then(|frame| frame.identity.as_ref()) != Some(&identity) {
            return self.handle_reopen_error(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::List)
                    .with_reason("directory identity changed while reopening walker frame")
                    .with_path(diagnostic_directory),
            );
        }
        let directory_permit = match self.acquire_host_directory(&directory) {
            Ok(permit) => permit,
            Err(error) => return self.handle_reopen_error(error),
        };
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => return self.handle_reopen_error(walk_io_error(&diagnostic_directory, error)),
        };
        let frame = self
            .stack
            .last_mut()
            .expect("reopening requires a non-empty walker stack");
        frame.entries = Some(entries);
        frame.directory_permit = directory_permit;
        Ok(())
    }

    /// Applies the configured error policy to a failed host frame reopen.
    fn handle_reopen_error(&mut self, error: LocalFileError) -> LocalResult<()> {
        if self.options.error_policy() == LocalWalkErrorPolicy::FailFast {
            self.terminated = true;
        } else {
            let removed = self.pop_host_frame();
            if let Some(identity) = removed.and_then(|frame| frame.identity) {
                self.followed_directories.remove(&identity);
            }
        }
        Err(error)
    }
}

impl LocalDirectoryWalker {
    /// Produces the next entry, opening at most one new directory as needed.
    fn next_entry(&mut self) -> Option<LocalResult<LocalDirectoryEntry>> {
        if self.terminated {
            return None;
        }
        if matches!(self.deadline, Some(deadline) if Instant::now() >= deadline) {
            self.terminated = true;
            return Some(Err(walk_io_error(
                &self.root,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "local listing deadline exceeded"),
            )));
        }
        if let Some(state) = self.rooted.as_mut() {
            let options = self.options;
            let pool = &self.open_directories;
            let namespace_root = self.root.clone();
            let result = next_rooted_entry(
                state,
                &namespace_root,
                options,
                pool,
                &mut self.entry_budget,
                &mut self.seen_name_budget,
                self.deadline,
            );
            if matches!(&result, Some(Err(error)) if is_terminal_walk_error(error, self.options.error_policy())) {
                self.terminated = true;
            }
            return result;
        }
        loop {
            let frame = self.stack.last()?;
            let entry_depth = frame.entry_depth;
            let relative_parent = frame.relative.clone();
            let needs_reopen = frame.entries.is_none();
            if needs_reopen && let Err(error) = self.reopen_host_frame(&relative_parent) {
                return Some(Err(error));
            }
            let frame = self.stack.last_mut()?;
            let next_entry = loop {
                let next = frame
                    .entries
                    .as_mut()
                    .expect("host walker reader was initialized")
                    .next();
                match next {
                    Some(Ok(entry)) => {
                        let name = entry.file_name();
                        if frame.seen.insert(name) {
                            break Some(Ok(entry));
                        }
                        continue;
                    }
                    other => break other,
                }
            };
            #[cfg(feature = "test-support")]
            let next_entry = if crate::local::take_test_support("walker-entry") {
                Some(Err(std::io::Error::other("injected walker directory entry failure")))
            } else {
                next_entry
            };
            let entry = match next_entry {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    if self.options.error_policy() == LocalWalkErrorPolicy::FailFast {
                        self.terminated = true;
                    }
                    return Some(Err(walk_io_error(&self.root.join(&relative_parent), error)));
                }
                None => {
                    let completed = self.pop_host_frame().expect("stack is non-empty");
                    if let Some(identity) = completed.identity {
                        self.followed_directories.remove(&identity);
                    }
                    continue;
                }
            };
            if self
                .options
                .max_depth()
                .is_some_and(|max_depth| entry_depth > max_depth)
            {
                continue;
            }
            if let Some(budget) = self.entry_budget.as_mut()
                && let Err(error) = budget.try_consume(1)
            {
                self.terminated = true;
                return Some(Err(directory_limit_error(&self.root.join(&relative_parent), error)));
            }
            if let Some(budget) = self.seen_name_budget.as_mut()
                && let Err(error) = budget.try_consume(name_bytes(&entry.file_name()))
            {
                self.terminated = true;
                return Some(Err(directory_limit_error(&self.root.join(&relative_parent), error)));
            }
            let path = entry.path();
            let relative = relative_parent.join(entry.file_name());
            let entry_metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    if self.options.error_policy() == LocalWalkErrorPolicy::FailFast {
                        self.terminated = true;
                    }
                    return Some(Err(walk_io_error(&path, error)));
                }
            };
            let descent_metadata = if entry_metadata.file_type().is_symlink() && self.symlink_policy.follows() {
                match fs::metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        if self.options.error_policy() == LocalWalkErrorPolicy::FailFast {
                            self.terminated = true;
                        }
                        return Some(Err(walk_io_error(&path, error)));
                    }
                }
            } else {
                entry_metadata.clone()
            };
            let is_directory = descent_metadata.file_type().is_dir();
            let metadata = LocalFileMetadata::from_native(&entry_metadata);

            if is_directory
                && self.may_descend(entry_depth)
                && let Err(error) = self.descend(&path, &descent_metadata, relative.clone(), entry_depth)
            {
                if self.options.error_policy() == LocalWalkErrorPolicy::FailFast {
                    self.terminated = true;
                }
                return Some(Err(error));
            }
            let namespace_path = self.root.join(&relative);
            return Some(Ok(LocalDirectoryEntry::new(
                namespace_path,
                relative,
                Some(path),
                metadata,
            )));
        }
    }
}

impl Iterator for LocalDirectoryWalker {
    /// Structured directory entry or traversal failure produced per step.
    type Item = LocalResult<LocalDirectoryEntry>;

    /// Advances the traversal while preserving its creation-time PWD context.
    fn next(&mut self) -> Option<Self::Item> {
        let current_directory = self.current_directory.clone();
        self.next_entry().map(|result| {
            result.map_err(|error| match current_directory {
                Some(current_directory) => error.with_current_directory(current_directory),
                None => error,
            })
        })
    }
}

/// Closes one rooted frame reader and drops its capacity permit.
///
/// # Parameters
///
/// - `frame`: Rooted frame whose optional reader is closed.
///
/// The reader is dropped before its permit so capacity never becomes available
/// while the rooted reader is still open.
fn close_rooted_frame(frame: &mut RootedWalkFrame) {
    let _ = frame.reader.take();
    let _ = frame.directory_permit.take();
}

/// Closes all rooted readers while retaining their traversal frames.
///
/// # Parameters
///
/// - `state`: Rooted traversal state containing retained frames.
fn close_all_rooted_frames(state: &mut RootedWalkState) {
    for frame in &mut state.stack {
        close_rooted_frame(frame);
    }
}

/// Pops one rooted frame after closing its reader, if present.
///
/// # Parameters
///
/// - `state`: Rooted traversal state containing the frame stack.
///
/// # Returns
///
/// Returns the removed frame, or `None` when traversal is complete.
fn pop_rooted_frame(state: &mut RootedWalkState) -> Option<RootedWalkFrame> {
    let mut frame = state.stack.pop()?;
    close_rooted_frame(&mut frame);
    Some(frame)
}

/// Acquires one rooted reader slot according to the configured policy.
///
/// # Parameters
///
/// - `state`: Rooted traversal state whose readers may be closed for reuse.
/// - `options`: Fixed traversal policy.
/// - `pool`: Shared open-directory occupancy pool.
/// - `path`: Diagnostic path attached to capacity errors.
///
/// # Errors
///
/// Returns [`LocalFileErrorKind::ResourceLimit`] when capacity is exhausted
/// under `Fail`. Under `Reopen`, every open rooted reader is closed before
/// acquisition is retried.
///
/// # Returns
///
/// A permit that remains owned for the opened reader's lifetime, or `None`
/// when the caller configured no open-directory limit.
fn acquire_rooted_directory(
    state: &mut RootedWalkState,
    options: LocalListOptions,
    pool: &Option<ManagedResourcePool<LocalResourceKind, usize>>,
    path: &Path,
) -> LocalResult<Option<ManagedResourcePermit<LocalResourceKind, usize>>> {
    let Some(directory_pool) = pool.as_ref() else {
        return Ok(None);
    };
    match directory_pool.try_acquire(1) {
        Ok(permit) => Ok(Some(permit)),
        Err(_error) if options.reopen_policy() == LocalDirectoryReopenPolicy::Reopen => {
            close_all_rooted_frames(state);
            match pool
                .as_ref()
                .expect("configured directory budget remains present")
                .try_acquire(1)
            {
                Ok(permit) => Ok(Some(permit)),
                Err(error) => Err(directory_limit_error(path, error)),
            }
        }
        Err(error) => Err(directory_limit_error(path, error)),
    }
}

/// Produces the next descriptor-relative rooted entry.
///
/// # Parameters
///
/// - `state`: Rooted traversal state.
/// - `options`: Fixed traversal policy.
/// - `pool`: Shared managed capacity for currently open rooted readers.
///
/// # Returns
///
/// The next structured entry or path-specific error, or `None` at completion.
fn next_rooted_entry(
    state: &mut RootedWalkState,
    namespace_root: &Path,
    options: LocalListOptions,
    pool: &Option<ManagedResourcePool<LocalResourceKind, usize>>,
    entry_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    seen_name_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    deadline: Option<Instant>,
) -> Option<LocalResult<LocalDirectoryEntry>> {
    loop {
        if matches!(deadline, Some(deadline) if Instant::now() >= deadline) {
            return Some(Err(walk_io_error(
                namespace_root,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "local listing deadline exceeded"),
            )));
        }
        let frame = state.stack.last()?;
        let entry_depth = frame.entry_depth;
        let authority_parent = frame.authority_parent.clone();
        let output_parent = frame.output_parent.clone();
        let needs_reader = frame.reader.is_none();
        if needs_reader {
            #[cfg(feature = "test-support")]
            let authority_parent = if crate::local::test_support_enabled("walker-rooted-relative-path") {
                PathBuf::from("../invalid")
            } else {
                authority_parent.clone()
            };
            let public_parent = namespace_root.join(&output_parent);
            let directory_permit = match acquire_rooted_directory(state, options, pool, &public_parent) {
                Ok(permit) => permit,
                Err(error) => {
                    let failed = pop_rooted_frame(state).expect("rooted walker stack is non-empty");
                    if let Some(identity) = failed.identity {
                        state.followed_directories.remove(&identity);
                    }
                    return Some(Err(error));
                }
            };
            let reader = if authority_parent.as_os_str().is_empty() {
                state.root.open_root_dir_reader()
            } else {
                let relative = match crate::local::LocalRelativePath::new(&authority_parent) {
                    Ok(relative) => relative,
                    Err(error) => {
                        let failed = pop_rooted_frame(state).expect("rooted walker stack is non-empty");
                        if let Some(identity) = failed.identity {
                            state.followed_directories.remove(&identity);
                        }
                        return Some(Err(walk_io_error(&public_parent, error)));
                    }
                };
                state.root.open_dir_reader(&relative)
            };
            match reader {
                Ok(reader) => {
                    let observed_identity = match reader
                        .try_clone_directory()
                        .and_then(|file| crate::rooted::Metadata::from_open_file(&file))
                        .map(|metadata| DirectoryIdentity::from_rooted_metadata(&metadata, &authority_parent))
                    {
                        Ok(identity) => identity,
                        Err(error) => {
                            let failed = pop_rooted_frame(state).expect("rooted walker stack is non-empty");
                            if let Some(identity) = failed.identity {
                                state.followed_directories.remove(&identity);
                            }
                            return Some(Err(walk_io_error(&public_parent, error)));
                        }
                    };
                    if state.stack.last().and_then(|frame| frame.identity.as_ref()) != Some(&observed_identity) {
                        let failed = pop_rooted_frame(state).expect("rooted walker stack is non-empty");
                        if let Some(identity) = failed.identity {
                            state.followed_directories.remove(&identity);
                        }
                        return Some(Err(LocalFileError::new(
                            LocalFileErrorKind::InvalidPath,
                            LocalFileOperation::List,
                        )
                        .with_reason("directory identity changed while reopening walker frame")
                        .with_path(public_parent)));
                    }
                    let frame = state.stack.last_mut().expect("rooted walker stack is non-empty");
                    frame.reader = Some(reader);
                    frame.directory_permit = directory_permit;
                }
                Err(error) => {
                    let failed = pop_rooted_frame(state).expect("rooted walker stack is non-empty");
                    if let Some(identity) = failed.identity {
                        state.followed_directories.remove(&identity);
                    }
                    return Some(Err(walk_io_error(&public_parent, error)));
                }
            }
        }
        let next_entry = state
            .stack
            .last_mut()
            .expect("rooted walker stack is non-empty")
            .reader
            .as_mut()
            .expect("rooted frame reader was initialized")
            .next_entry();
        let entry = match next_entry {
            Ok(Some(entry)) => {
                if !state
                    .stack
                    .last_mut()
                    .expect("rooted walker stack is non-empty")
                    .seen
                    .insert(entry.name().to_os_string())
                {
                    continue;
                }
                entry
            }
            Ok(None) => {
                if let Some(frame) = pop_rooted_frame(state)
                    && let Some(identity) = frame.identity
                {
                    state.followed_directories.remove(&identity);
                }
                continue;
            }
            Err(error) => {
                if let Some(frame) = pop_rooted_frame(state)
                    && let Some(identity) = frame.identity
                {
                    state.followed_directories.remove(&identity);
                }
                return Some(Err(walk_io_error(&namespace_root.join(&output_parent), error)));
            }
        };
        if options.max_depth().is_some_and(|max_depth| entry_depth > max_depth) {
            continue;
        }
        if let Some(budget) = entry_budget.as_mut()
            && let Err(error) = budget.try_consume(1)
        {
            return Some(Err(directory_limit_error(namespace_root, error)));
        }
        if let Some(budget) = seen_name_budget.as_mut()
            && let Err(error) = budget.try_consume(name_bytes(entry.name()))
        {
            return Some(Err(directory_limit_error(namespace_root, error)));
        }
        let authority_path = authority_parent.join(entry.name());
        let output_path = output_parent.join(entry.name());
        let entry_metadata = entry.metadata();
        let metadata = crate::rooted_local_file_system::rooted_metadata(entry_metadata);
        let may_descend = options.recursive() && options.max_depth().is_none_or(|max_depth| entry_depth < max_depth);
        let mut followed_directory = None;
        if metadata.kind() == crate::LocalFileKind::Directory {
            let identity = DirectoryIdentity::from_rooted_metadata(&entry_metadata, &authority_path);
            if state.followed_directories.contains(&identity) {
                return Some(Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidPath,
                    LocalFileOperation::List,
                )
                .with_reason("directory identity cycle detected")
                .with_path(namespace_root.join(&output_path))));
            }
            followed_directory = Some((authority_path.clone(), identity));
        } else if metadata.kind() == crate::LocalFileKind::Symlink && state.symlink_policy.follows() && may_descend {
            let target = match crate::rooted_local_file_system::resolve_rooted_path_allow_root(
                &state.root,
                &authority_path,
                state.symlink_policy,
                true,
                LocalFileOperation::List,
            ) {
                Ok(target) => target,
                Err(error) => {
                    return Some(Err(error.with_path(namespace_root.join(&output_path))));
                }
            };
            let target_metadata = if target.as_os_str().is_empty() {
                state.root.metadata()
            } else {
                match crate::local::LocalRelativePath::new(&target) {
                    Ok(relative) => state.root.symlink_metadata(&relative),
                    Err(error) => Err(error),
                }
            };
            let target_metadata = match target_metadata {
                Ok(metadata) => metadata,
                Err(error) => {
                    return Some(Err(walk_io_error(&namespace_root.join(&output_path), error)));
                }
            };
            if target_metadata.kind() == crate::rooted::EntryKind::Directory {
                let identity = DirectoryIdentity::from_rooted_metadata(&target_metadata, &target);
                if state.followed_directories.contains(&identity) {
                    return Some(Err(LocalFileError::new(
                        LocalFileErrorKind::InvalidPath,
                        LocalFileOperation::List,
                    )
                    .with_reason("symbolic-link directory cycle detected")
                    .with_path(namespace_root.join(&output_path))));
                }
                followed_directory = Some((target, identity));
            }
        }
        if may_descend && let Some((authority_parent, identity)) = followed_directory {
            state.followed_directories.insert(identity.clone());
            let (authority_parent, identity) = (authority_parent, Some(identity));
            state.stack.push(RootedWalkFrame {
                reader: None,
                directory_permit: None,
                seen: std::collections::HashSet::new(),
                authority_parent,
                output_parent: output_path.clone(),
                entry_depth: entry_depth + 1,
                identity,
            });
        }
        let diagnostic_path = state.root.path().join(&authority_path);
        return Some(Ok(LocalDirectoryEntry::new(
            namespace_root.join(&output_path),
            output_path,
            Some(diagnostic_path),
            metadata,
        )));
    }
}

/// Builds a stable native identity for one directory path.
///
/// # Parameters
///
/// - `metadata`: Metadata observed for the directory.
/// - `path`: Directory path used by platforms without directly available
///   identity fields.
///
/// # Returns
///
/// A directory identity suitable for active-path cycle detection.
///
/// # Errors
///
/// Returns `LocalFileError` when the platform requires canonical path
/// resolution and that resolution fails.
fn native_directory_identity(metadata: &fs::Metadata, path: &Path) -> LocalResult<DirectoryIdentity> {
    #[cfg(windows)]
    let identity_path = fs::canonicalize(path).map_err(|error| walk_io_error(path, error))?;
    #[cfg(not(windows))]
    let identity_path = path;
    #[cfg(windows)]
    let identity = DirectoryIdentity::from_metadata(metadata, &identity_path);
    #[cfg(not(windows))]
    let identity = DirectoryIdentity::from_metadata(metadata, identity_path);
    Ok(identity)
}

/// Adds an offending traversal path to a native I/O failure.
///
/// # Parameters
///
/// - `path`: Path being traversed.
/// - `error`: Native I/O failure.
///
/// # Returns
///
/// Structured listing error.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn walk_io_error(path: &Path, error: std::io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::List, Some(path.to_path_buf()), None, error)
}
