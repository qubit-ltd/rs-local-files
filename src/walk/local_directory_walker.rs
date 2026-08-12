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

use qubit_budget::BudgetError;
use qubit_budget::ResourceBudget;
use qubit_budget::ResourcePool;

use super::internal::RootedWalkFrame;
use super::internal::RootedWalkState;
use super::internal::WalkFrame;
use crate::LocalDirectoryEntry;
use crate::LocalDirectoryReopenPolicy;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalListOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWalkErrorPolicy;
use crate::local::DirectoryIdentity;

/// Lazy depth-first iterator over native local directory entries.
#[derive(Debug)]
pub struct LocalDirectoryWalker {
    /// Bound traversal root.
    root: PathBuf,
    /// Policy fixed when the walker is created.
    options: LocalListOptions,
    /// Open directory iterators, bounded by traversal depth.
    stack: Vec<WalkFrame>,
    /// Current number of open native directory readers.
    open_directories: ResourcePool<LocalResourceKind, usize>,
    /// Native directory identities on the active DFS path.
    followed_directories: HashSet<DirectoryIdentity>,
    /// Descriptor-relative traversal state for a rooted walker.
    rooted: Option<RootedWalkState>,
    /// Whether fail-fast error policy has terminated iteration.
    terminated: bool,
    /// Symbolic-link policy fixed when the walker is created.
    symlink_policy: LocalSymlinkPolicy,
    entry_budget: Option<ResourceBudget<LocalResourceKind, usize>>,
    seen_name_budget: Option<ResourceBudget<LocalResourceKind, usize>>,
    deadline: Option<Instant>,
}

impl LocalDirectoryWalker {
    /// Opens a lazy walker rooted at a bound native directory.
    ///
    /// # Parameters
    ///
    /// - `root`: Bound absolute traversal root.
    /// - `options`: Traversal policy fixed for the walker lifetime.
    ///
    /// # Returns
    ///
    /// A walker that opens descendants only as iteration advances.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the root is not a directory or cannot be
    /// opened.
    pub(crate) fn open(
        root: PathBuf,
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        validate_options(&root, &options)?;
        let metadata = fs::symlink_metadata(&root)
            .map_err(|error| walk_io_error(&root, error))?;
        if !metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::List,
            )
            .with_path(root));
        }
        let mut open_directories = directory_pool(&options);
        open_directories
            .try_acquire(1)
            .expect("validated non-zero directory capacity accepts root");
        let entries =
            fs::read_dir(&root).map_err(|error| walk_io_error(&root, error))?;
        #[cfg(feature = "internal-test-support")]
        if crate::local::test_support_enabled("walker-root-canonicalize") {
            return Err(walk_io_error(
                &root,
                std::io::Error::other(
                    "injected walker root canonicalization failure",
                ),
            ));
        }
        let root_identity = native_directory_identity(&metadata, &root)?;
        let mut followed_directories = HashSet::new();
        followed_directories.insert(root_identity.clone());
        Ok(Self {
            root,
            options,
            stack: vec![WalkFrame {
                entries: Some(entries),
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
            entry_budget: options.max_entries().map(|limit| {
                ResourceBudget::new(LocalResourceKind::Entry, limit)
            }),
            seen_name_budget: options.max_seen_name_bytes().map(|limit| {
                ResourceBudget::new(LocalResourceKind::SeenNameBytes, limit)
            }),
            deadline: options
                .deadline()
                .map(|duration| Instant::now() + duration),
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
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        Self::open_rooted_with_output(
            root,
            path,
            PathBuf::new(),
            options,
            symlink_policy,
        )
    }

    /// Creates a rooted walker with separate authority and logical output
    /// paths, which preserves a symlink component in returned paths.
    pub(crate) fn open_rooted_with_output(
        root: Arc<crate::rooted::Root>,
        path: Option<crate::local::LocalRelativePath>,
        output_parent: PathBuf,
        options: LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let diagnostic_root = root
            .path()
            .join(path.as_ref().map_or_else(PathBuf::new, |path| {
                path.as_path().to_path_buf()
            }));
        validate_options(&diagnostic_root, &options)?;
        let authority_parent = path
            .as_ref()
            .map_or_else(PathBuf::new, |path| path.as_path().to_path_buf());
        let authority_root = root
            .authority_path()
            .map_err(|error| walk_io_error(&diagnostic_root, error))?;
        let authority_start = authority_root.join(&authority_parent);
        let start_metadata = fs::metadata(&authority_start)
            .map_err(|error| walk_io_error(&diagnostic_root, error))?;
        let start_identity =
            native_directory_identity(&start_metadata, &authority_start)?;
        let mut followed_directories = HashSet::new();
        followed_directories.insert(start_identity.clone());
        Ok(Self {
            root: diagnostic_root,
            options,
            stack: Vec::new(),
            open_directories: directory_pool(&options),
            followed_directories: HashSet::new(),
            rooted: Some(RootedWalkState {
                root,
                stack: vec![RootedWalkFrame {
                    reader: None,
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
            entry_budget: options.max_entries().map(|limit| {
                ResourceBudget::new(LocalResourceKind::Entry, limit)
            }),
            seen_name_budget: options.max_seen_name_bytes().map(|limit| {
                ResourceBudget::new(LocalResourceKind::SeenNameBytes, limit)
            }),
            deadline: options
                .deadline()
                .map(|duration| Instant::now() + duration),
        })
    }

    /// Returns the bound traversal root.
    #[must_use]
    #[inline(always)]
    pub fn root(&self) -> &Path {
        &self.root
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
    #[inline(always)]
    fn may_descend(&self, entry_depth: usize) -> bool {
        self.options.recursive()
            && self
                .options
                .max_depth()
                .is_none_or(|max_depth| entry_depth < max_depth)
    }

    /// Closes every currently open host reader while retaining its frame.
    ///
    /// Each closed reader releases exactly one acquired directory slot. The
    /// retained frames can later be reopened without changing DFS state.
    fn close_all_host_frames(&mut self) {
        for frame in &mut self.stack {
            close_host_frame(frame, &mut self.open_directories);
        }
    }

    /// Pops one host frame after closing its reader, if present.
    ///
    /// # Returns
    ///
    /// Returns the removed frame, or `None` when traversal has no host frame.
    fn pop_host_frame(&mut self) -> Option<WalkFrame> {
        let mut frame = self.stack.pop()?;
        close_host_frame(&mut frame, &mut self.open_directories);
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
    fn acquire_host_directory(&mut self, path: &Path) -> LocalResult<()> {
        match self.open_directories.try_acquire(1) {
            Ok(()) => Ok(()),
            Err(_error)
                if self.options.reopen_policy()
                    == LocalDirectoryReopenPolicy::Reopen =>
            {
                self.close_all_host_frames();
                self.open_directories
                    .try_acquire(1)
                    .map_err(|error| directory_limit_error(path, error))
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
        #[cfg(feature = "internal-test-support")]
        if crate::local::test_support_enabled("walker-descend-canonicalize") {
            return Err(walk_io_error(
                path,
                std::io::Error::other(
                    "injected walker descent canonicalization failure",
                ),
            ));
        }
        let identity = native_directory_identity(metadata, path)?;
        if self.followed_directories.contains(&identity) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidPath,
                LocalFileOperation::List,
            )
            .with_reason("directory identity cycle detected")
            .with_path(path.to_path_buf()));
        }
        self.acquire_host_directory(path)?;
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                self.open_directories
                    .release(1)
                    .expect("failed open had reserved one directory slot");
                return Err(walk_io_error(path, error));
            }
        };
        self.followed_directories.insert(identity.clone());
        self.stack.push(WalkFrame {
            entries: Some(entries),
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
        let directory = self.root.join(relative_parent);
        #[cfg(feature = "internal-test-support")]
        if crate::local::test_support_enabled("walker-reopen-canonicalize") {
            return self.handle_reopen_error(walk_io_error(
                &directory,
                std::io::Error::other(
                    "injected walker reopen canonicalization failure",
                ),
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
                    LocalFileError::new(
                        LocalFileErrorKind::InvalidPath,
                        LocalFileOperation::List,
                    )
                    .with_reason(
                        "directory entry changed while reopening walker frame",
                    )
                    .with_path(directory),
                );
            }
            Err(error) => {
                return self
                    .handle_reopen_error(walk_io_error(&directory, error));
            }
        };
        let identity = match native_directory_identity(&metadata, &directory) {
            Ok(identity) => identity,
            Err(error) => return self.handle_reopen_error(error),
        };
        if self.stack.last().and_then(|frame| frame.identity.as_ref())
            != Some(&identity)
        {
            return self.handle_reopen_error(
                LocalFileError::new(
                    LocalFileErrorKind::InvalidPath,
                    LocalFileOperation::List,
                )
                .with_reason(
                    "directory identity changed while reopening walker frame",
                )
                .with_path(directory),
            );
        }
        if let Err(error) = self.acquire_host_directory(&directory) {
            return self.handle_reopen_error(error);
        }
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                self.open_directories
                    .release(1)
                    .expect("failed reopen had reserved one directory slot");
                return self
                    .handle_reopen_error(walk_io_error(&directory, error));
            }
        };
        self.stack
            .last_mut()
            .expect("reopening requires a non-empty walker stack")
            .entries = Some(entries);
        Ok(())
    }

    /// Applies the configured error policy to a failed host frame reopen.
    fn handle_reopen_error(
        &mut self,
        error: LocalFileError,
    ) -> LocalResult<()> {
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

/// Closes one host frame reader and returns its pool capacity.
///
/// # Parameters
///
/// - `frame`: Host frame whose optional reader is closed.
/// - `pool`: Pool that recorded every open host reader.
///
/// This function updates the frame and pool together. It panics only when
/// their internal occupancy invariant was already violated.
fn close_host_frame(
    frame: &mut WalkFrame,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
) {
    if frame.entries.take().is_some() {
        pool.release(1)
            .expect("one host reader was recorded as open");
    }
}

/// Creates the established listing error for exhausted directory capacity.
///
/// # Parameters
///
/// - `path`: Directory that could not obtain an open-reader slot.
///
/// # Returns
///
/// A [`LocalFileErrorKind::ResourceLimit`] error carrying the listing
/// operation, path, and complete budget facts.
#[must_use]
fn directory_limit_error(
    path: &Path,
    error: BudgetError<LocalResourceKind, usize>,
) -> LocalFileError {
    match error {
        BudgetError::Insufficient {
            resource,
            limit,
            remaining,
            requested,
        } => LocalFileError::from_resource_limit(
            LocalFileOperation::List,
            Some(path.to_path_buf()),
            LocalResourceLimitError::new(resource, limit, remaining, requested),
        ),
        BudgetError::LimitExceeded { .. } => LocalFileError::new(
            LocalFileErrorKind::ResourceLimit,
            LocalFileOperation::List,
        )
        .with_path(path.to_path_buf()),
    }
}

/// Validates options that must hold before a walker can be constructed.
///
/// # Parameters
///
/// - `root`: Diagnostic traversal root.
/// - `options`: Traversal policy to validate.
///
/// # Errors
///
/// Returns `InvalidOptions` when the open-directory budget is zero.
fn validate_options(
    root: &Path,
    options: &LocalListOptions,
) -> LocalResult<()> {
    if options.max_open_directories() == 0 {
        return Err(LocalFileError::new(
            LocalFileErrorKind::InvalidOptions,
            LocalFileOperation::List,
        )
        .with_path(root.to_path_buf())
        .with_reason(
            "maximum open directory count must be greater than zero",
        ));
    }
    Ok(())
}

/// Creates the finite pool that accounts for opened directory readers.
fn directory_pool(
    options: &LocalListOptions,
) -> ResourcePool<LocalResourceKind, usize> {
    ResourcePool::new(
        LocalResourceKind::OpenDirectory,
        options.max_open_directories(),
    )
}

fn name_bytes(name: &std::ffi::OsStr) -> usize {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes().len()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().count().saturating_mul(2)
    }
    #[cfg(not(any(unix, windows)))]
    {
        name.to_string_lossy().len()
    }
}

impl Iterator for LocalDirectoryWalker {
    type Item = LocalResult<LocalDirectoryEntry>;

    /// Produces the next entry, opening at most one new directory as needed.
    fn next(&mut self) -> Option<Self::Item> {
        if self.terminated {
            return None;
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.terminated = true;
            return Some(Err(walk_io_error(
                &self.root,
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "local listing deadline exceeded",
                ),
            )));
        }
        if let Some(state) = self.rooted.as_mut() {
            let options = self.options;
            let pool = &mut self.open_directories;
            let result = next_rooted_entry(
                state,
                options,
                pool,
                &mut self.entry_budget,
                &mut self.seen_name_budget,
                self.deadline,
            );
            if matches!(&result, Some(Err(_)))
                && self.options.error_policy() == LocalWalkErrorPolicy::FailFast
            {
                self.terminated = true;
            }
            return result;
        }
        loop {
            let frame = self.stack.last()?;
            let entry_depth = frame.entry_depth;
            let relative_parent = frame.relative.clone();
            let needs_reopen = frame.entries.is_none();
            if needs_reopen
                && let Err(error) = self.reopen_host_frame(&relative_parent)
            {
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
            #[cfg(feature = "internal-test-support")]
            let next_entry = if crate::local::take_test_support("walker-entry")
            {
                Some(Err(std::io::Error::other(
                    "injected walker directory entry failure",
                )))
            } else {
                next_entry
            };
            let entry = match next_entry {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    if self.options.error_policy()
                        == LocalWalkErrorPolicy::FailFast
                    {
                        self.terminated = true;
                    }
                    return Some(Err(walk_io_error(
                        &self.root.join(&relative_parent),
                        error,
                    )));
                }
                None => {
                    let completed =
                        self.pop_host_frame().expect("stack is non-empty");
                    if let Some(identity) = completed.identity {
                        self.followed_directories.remove(&identity);
                    }
                    continue;
                }
            };
            if let Some(budget) = self.entry_budget.as_mut()
                && let Err(error) = budget.try_consume(1)
            {
                self.terminated = self.options.error_policy()
                    == LocalWalkErrorPolicy::FailFast;
                return Some(Err(directory_limit_error(
                    &self.root.join(&relative_parent),
                    error,
                )));
            }
            if let Some(budget) = self.seen_name_budget.as_mut()
                && let Err(error) =
                    budget.try_consume(name_bytes(&entry.file_name()))
            {
                self.terminated = self.options.error_policy()
                    == LocalWalkErrorPolicy::FailFast;
                return Some(Err(directory_limit_error(
                    &self.root.join(&relative_parent),
                    error,
                )));
            }
            if self
                .options
                .max_depth()
                .is_some_and(|max_depth| entry_depth > max_depth)
            {
                continue;
            }

            let path = entry.path();
            let relative = relative_parent.join(entry.file_name());
            let native_metadata = if self.symlink_policy.follows() {
                fs::metadata(&path)
            } else {
                fs::symlink_metadata(&path)
            };
            let native_metadata = match native_metadata {
                Ok(metadata) => metadata,
                Err(error) => {
                    if self.options.error_policy()
                        == LocalWalkErrorPolicy::FailFast
                    {
                        self.terminated = true;
                    }
                    return Some(Err(walk_io_error(&path, error)));
                }
            };
            let is_directory = native_metadata.file_type().is_dir();
            let metadata = LocalFileMetadata::from_native(&native_metadata);

            if is_directory
                && self.may_descend(entry_depth)
                && let Err(error) = self.descend(
                    &path,
                    &native_metadata,
                    relative.clone(),
                    entry_depth,
                )
            {
                if self.options.error_policy() == LocalWalkErrorPolicy::FailFast
                {
                    self.terminated = true;
                }
                return Some(Err(error));
            }
            return Some(Ok(LocalDirectoryEntry::new(
                path, relative, metadata,
            )));
        }
    }
}

/// Closes one rooted frame reader and returns its pool capacity.
///
/// # Parameters
///
/// - `frame`: Rooted frame whose optional reader is closed.
/// - `pool`: Pool that recorded every open rooted reader.
///
/// This function updates the frame and pool together. It panics only when
/// their internal occupancy invariant was already violated.
fn close_rooted_frame(
    frame: &mut RootedWalkFrame,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
) {
    if frame.reader.take().is_some() {
        pool.release(1)
            .expect("one rooted reader was recorded as open");
    }
}

/// Closes all rooted readers while retaining their traversal frames.
///
/// # Parameters
///
/// - `state`: Rooted traversal state containing retained frames.
/// - `pool`: Pool that recorded every open rooted reader.
fn close_all_rooted_frames(
    state: &mut RootedWalkState,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
) {
    for frame in &mut state.stack {
        close_rooted_frame(frame, pool);
    }
}

/// Pops one rooted frame after closing its reader, if present.
///
/// # Parameters
///
/// - `state`: Rooted traversal state containing the frame stack.
/// - `pool`: Pool that recorded every open rooted reader.
///
/// # Returns
///
/// Returns the removed frame, or `None` when traversal is complete.
fn pop_rooted_frame(
    state: &mut RootedWalkState,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
) -> Option<RootedWalkFrame> {
    let mut frame = state.stack.pop()?;
    close_rooted_frame(&mut frame, pool);
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
fn acquire_rooted_directory(
    state: &mut RootedWalkState,
    options: LocalListOptions,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
    path: &Path,
) -> LocalResult<()> {
    match pool.try_acquire(1) {
        Ok(()) => Ok(()),
        Err(_error)
            if options.reopen_policy()
                == LocalDirectoryReopenPolicy::Reopen =>
        {
            close_all_rooted_frames(state, pool);
            pool.try_acquire(1)
                .map_err(|error| directory_limit_error(path, error))
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
/// - `pool`: Shared occupancy for currently open rooted readers.
///
/// # Returns
///
/// The next structured entry or path-specific error, or `None` at completion.
fn next_rooted_entry(
    state: &mut RootedWalkState,
    options: LocalListOptions,
    pool: &mut ResourcePool<LocalResourceKind, usize>,
    entry_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    seen_name_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    deadline: Option<Instant>,
) -> Option<LocalResult<LocalDirectoryEntry>> {
    loop {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Some(Err(walk_io_error(
                state.root.path(),
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "local listing deadline exceeded",
                ),
            )));
        }
        let frame = state.stack.last()?;
        let entry_depth = frame.entry_depth;
        let authority_parent = frame.authority_parent.clone();
        let output_parent = frame.output_parent.clone();
        let needs_reader = frame.reader.is_none();
        if needs_reader {
            #[cfg(feature = "internal-test-support")]
            let authority_parent = if crate::local::test_support_enabled(
                "walker-rooted-relative-path",
            ) {
                PathBuf::from("../invalid")
            } else {
                authority_parent.clone()
            };
            let diagnostic_path = state.root.path().join(&authority_parent);
            if let Err(error) =
                acquire_rooted_directory(state, options, pool, &diagnostic_path)
            {
                let failed = pop_rooted_frame(state, pool)
                    .expect("rooted walker stack is non-empty");
                if let Some(identity) = failed.identity {
                    state.followed_directories.remove(&identity);
                }
                return Some(Err(error));
            }
            let reader = if authority_parent.as_os_str().is_empty() {
                state.root.open_root_dir_reader()
            } else {
                let relative = match crate::local::LocalRelativePath::new(
                    &authority_parent,
                ) {
                    Ok(relative) => relative,
                    Err(error) => {
                        pool.release(1).expect(
                            "invalid rooted path had reserved one rooted slot",
                        );
                        let failed = pop_rooted_frame(state, pool)
                            .expect("rooted walker stack is non-empty");
                        if let Some(identity) = failed.identity {
                            state.followed_directories.remove(&identity);
                        }
                        return Some(Err(walk_io_error(
                            &authority_parent,
                            error,
                        )));
                    }
                };
                state.root.open_dir_reader(&relative)
            };
            match reader {
                Ok(reader) => {
                    let observed_identity = match reader
                        .try_clone_directory()
                        .and_then(|file| file.metadata())
                        .and_then(|metadata| {
                            native_directory_identity(
                                &metadata,
                                &diagnostic_path,
                            )
                            .map_err(|error| {
                                std::io::Error::other(error.to_string())
                            })
                        }) {
                        Ok(identity) => identity,
                        Err(error) => {
                            pool.release(1).expect(
                                "failed identity check held one rooted slot",
                            );
                            let failed = pop_rooted_frame(state, pool)
                                .expect("rooted walker stack is non-empty");
                            if let Some(identity) = failed.identity {
                                state.followed_directories.remove(&identity);
                            }
                            return Some(Err(walk_io_error(
                                &diagnostic_path,
                                error,
                            )));
                        }
                    };
                    if state
                        .stack
                        .last()
                        .and_then(|frame| frame.identity.as_ref())
                        != Some(&observed_identity)
                    {
                        pool.release(1)
                            .expect("identity mismatch held one rooted slot");
                        let failed = pop_rooted_frame(state, pool)
                            .expect("rooted walker stack is non-empty");
                        if let Some(identity) = failed.identity {
                            state.followed_directories.remove(&identity);
                        }
                        return Some(Err(LocalFileError::new(
                            LocalFileErrorKind::InvalidPath,
                            LocalFileOperation::List,
                        )
                        .with_reason("directory identity changed while reopening walker frame")
                        .with_path(authority_parent.clone())));
                    }
                    state
                        .stack
                        .last_mut()
                        .expect("rooted walker stack is non-empty")
                        .reader = Some(reader);
                }
                Err(error) => {
                    pool.release(1)
                        .expect("failed open had reserved one rooted slot");
                    let failed = pop_rooted_frame(state, pool)
                        .expect("rooted walker stack is non-empty");
                    if let Some(identity) = failed.identity {
                        state.followed_directories.remove(&identity);
                    }
                    return Some(Err(walk_io_error(&authority_parent, error)));
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
                if let Some(frame) = pop_rooted_frame(state, pool)
                    && let Some(identity) = frame.identity
                {
                    state.followed_directories.remove(&identity);
                }
                continue;
            }
            Err(error) => {
                if let Some(frame) = pop_rooted_frame(state, pool)
                    && let Some(identity) = frame.identity
                {
                    state.followed_directories.remove(&identity);
                }
                return Some(Err(walk_io_error(&authority_parent, error)));
            }
        };
        if let Some(budget) = entry_budget.as_mut()
            && let Err(error) = budget.try_consume(1)
        {
            return Some(Err(directory_limit_error(state.root.path(), error)));
        }
        if let Some(budget) = seen_name_budget.as_mut()
            && let Err(error) = budget.try_consume(name_bytes(entry.name()))
        {
            return Some(Err(directory_limit_error(state.root.path(), error)));
        }
        if options
            .max_depth()
            .is_some_and(|max_depth| entry_depth > max_depth)
        {
            continue;
        }
        let authority_path = authority_parent.join(entry.name());
        let output_path = output_parent.join(entry.name());
        let mut metadata =
            crate::rooted_local_file_system::rooted_metadata(entry.metadata());
        let mut followed_directory = None;
        if metadata.kind() == crate::LocalFileKind::Symlink
            && state.symlink_policy.follows()
        {
            let authority_root = match state.root.authority_path() {
                Ok(path) => path,
                Err(error) => {
                    return Some(Err(walk_io_error(
                        &state.root.path().join(&authority_path),
                        error,
                    )));
                }
            };
            let diagnostic_path = state.root.path().join(&authority_path);
            let authority_target = authority_root.join(&authority_path);
            let target = match fs::canonicalize(&authority_target) {
                Ok(target) => target,
                Err(error) => {
                    return Some(Err(walk_io_error(&diagnostic_path, error)));
                }
            };
            let root = match fs::canonicalize(&authority_root) {
                Ok(root) => root,
                Err(error) => {
                    return Some(Err(walk_io_error(&diagnostic_path, error)));
                }
            };
            if !target.starts_with(&root) {
                return Some(Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidPath,
                    LocalFileOperation::List,
                )
                .with_reason(
                    "symbolic-link resolution escaped the rooted scope",
                )
                .with_path(diagnostic_path)));
            }
            let target_metadata = match fs::metadata(&target) {
                Ok(metadata) => metadata,
                Err(error) => {
                    return Some(Err(walk_io_error(&target, error)));
                }
            };
            metadata = LocalFileMetadata::from_native(&target_metadata);
            if metadata.kind() == crate::LocalFileKind::Directory {
                let identity = match native_directory_identity(
                    &target_metadata,
                    &target,
                ) {
                    Ok(identity) => identity,
                    Err(error) => return Some(Err(error)),
                };
                if state.followed_directories.contains(&identity) {
                    return Some(Err(LocalFileError::new(
                        LocalFileErrorKind::InvalidPath,
                        LocalFileOperation::List,
                    )
                    .with_reason("symbolic-link directory cycle detected")
                    .with_path(diagnostic_path)));
                }
                let relative_target = match target.strip_prefix(&root) {
                    Ok(relative) => relative.to_path_buf(),
                    Err(_) => unreachable!("target was checked within root"),
                };
                followed_directory = Some((relative_target, identity));
            }
        }
        let is_directory = metadata.kind() == crate::LocalFileKind::Directory;
        if is_directory && followed_directory.is_none() {
            let authority_root = match state.root.authority_path() {
                Ok(path) => path,
                Err(error) => {
                    return Some(Err(walk_io_error(
                        &state.root.path().join(&authority_path),
                        error,
                    )));
                }
            };
            let authority_target = authority_root.join(&authority_path);
            let target_metadata = match fs::metadata(&authority_target) {
                Ok(metadata) => metadata,
                Err(error) => {
                    return Some(Err(walk_io_error(
                        &state.root.path().join(&authority_path),
                        error,
                    )));
                }
            };
            let identity = match native_directory_identity(
                &target_metadata,
                &authority_target,
            ) {
                Ok(identity) => identity,
                Err(error) => return Some(Err(error)),
            };
            if state.followed_directories.contains(&identity) {
                return Some(Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidPath,
                    LocalFileOperation::List,
                )
                .with_reason("directory identity cycle detected")
                .with_path(state.root.path().join(&authority_path))));
            }
            followed_directory = Some((authority_path.clone(), identity));
        }
        let may_descend = if options.recursive() {
            match options.max_depth() {
                Some(max_depth) => entry_depth < max_depth,
                None => true,
            }
        } else {
            false
        };
        if is_directory && may_descend {
            let (authority_parent, identity) = followed_directory.map_or(
                (authority_path.clone(), None),
                |(target, identity)| {
                    state.followed_directories.insert(identity.clone());
                    (target, Some(identity))
                },
            );
            state.stack.push(RootedWalkFrame {
                reader: None,
                seen: std::collections::HashSet::new(),
                authority_parent,
                output_parent: output_path.clone(),
                entry_depth: entry_depth + 1,
                identity,
            });
        }
        let diagnostic_path = state.root.path().join(&authority_path);
        return Some(Ok(LocalDirectoryEntry::new(
            diagnostic_path,
            output_path,
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
fn native_directory_identity(
    metadata: &fs::Metadata,
    path: &Path,
) -> LocalResult<DirectoryIdentity> {
    #[cfg(windows)]
    let identity_path =
        fs::canonicalize(path).map_err(|error| walk_io_error(path, error))?;
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
#[must_use]
#[inline]
fn walk_io_error(path: &Path, error: std::io::Error) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::List,
        Some(path.to_path_buf()),
        None,
        error,
    )
}
