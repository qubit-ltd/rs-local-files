// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    collections::HashSet,
    fs,
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

use crate::{
    LocalDirectoryEntry,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalListOptions,
    LocalResult,
    LocalSymlinkPolicy,
    LocalWalkErrorPolicy,
};

// qubit-style: allow coverage-cfg

use super::internal::{
    RootedWalkFrame,
    RootedWalkState,
    WalkFrame,
};

/// Lazy depth-first iterator over native local directory entries.
#[derive(Debug)]
pub struct LocalDirectoryWalker {
    /// Bound traversal root.
    root: PathBuf,
    /// Policy fixed when the walker is created.
    options: LocalListOptions,
    /// Open directory iterators, bounded by traversal depth.
    stack: Vec<WalkFrame>,
    /// Canonical directory identities on the active follow-mode DFS path.
    followed_directories: HashSet<PathBuf>,
    /// Descriptor-relative traversal state for a rooted walker.
    rooted: Option<RootedWalkState>,
    /// Whether fail-fast error policy has terminated iteration.
    terminated: bool,
    /// Symbolic-link policy fixed when the walker is created.
    symlink_policy: LocalSymlinkPolicy,
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
        let entries =
            fs::read_dir(&root).map_err(|error| walk_io_error(&root, error))?;
        let mut followed_directories = HashSet::new();
        let root_identity = if symlink_policy.follows() {
            #[cfg(coverage)]
            if crate::local::coverage_fault_enabled("walker-root-canonicalize")
            {
                return Err(walk_io_error(
                    &root,
                    std::io::Error::other(
                        "injected walker root canonicalization failure",
                    ),
                ));
            }
            let identity = match fs::canonicalize(&root) {
                Ok(identity) => identity,
                Err(error) => return Err(walk_io_error(&root, error)),
            };
            followed_directories.insert(identity.clone());
            Some(identity)
        } else {
            None
        };
        Ok(Self {
            root,
            options,
            stack: vec![WalkFrame {
                entries: Some(entries),
                seen: HashSet::new(),
                relative: PathBuf::new(),
                identity: root_identity,
                entry_depth: 1,
            }],
            followed_directories,
            rooted: None,
            terminated: false,
            symlink_policy,
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
        Ok(Self {
            root: diagnostic_root,
            options,
            stack: Vec::new(),
            followed_directories: HashSet::new(),
            rooted: Some(RootedWalkState {
                root,
                stack: vec![RootedWalkFrame {
                    reader: None,
                    seen: HashSet::new(),
                    authority_parent,
                    output_parent,
                    entry_depth: 1,
                    identity: None,
                }],
                followed_directories: HashSet::new(),
                symlink_policy,
            }),
            terminated: false,
            symlink_policy,
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
        relative: PathBuf,
        entry_depth: usize,
    ) -> LocalResult<()> {
        if self.stack.len() >= self.options.max_open_directories()
            && self.options.reopen_policy()
                == crate::LocalDirectoryReopenPolicy::Fail
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ResourceLimit,
                LocalFileOperation::List,
            )
            .with_path(path.to_path_buf()));
        }
        if self.stack.len() >= self.options.max_open_directories() {
            for frame in &mut self.stack {
                frame.entries = None;
            }
        }
        let identity = if self.symlink_policy.follows() {
            #[cfg(coverage)]
            if crate::local::coverage_fault_enabled(
                "walker-descend-canonicalize",
            ) {
                return Err(walk_io_error(
                    path,
                    std::io::Error::other(
                        "injected walker descent canonicalization failure",
                    ),
                ));
            }
            let identity = match fs::canonicalize(path) {
                Ok(identity) => identity,
                Err(error) => return Err(walk_io_error(path, error)),
            };
            if self.followed_directories.contains(&identity) {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidPath,
                    LocalFileOperation::List,
                )
                .with_path(path.to_path_buf()));
            }
            Some(identity)
        } else {
            None
        };
        let entries =
            fs::read_dir(path).map_err(|error| walk_io_error(path, error))?;
        if let Some(identity) = identity.as_ref() {
            self.followed_directories.insert(identity.clone());
        }
        self.stack.push(WalkFrame {
            entries: Some(entries),
            seen: HashSet::new(),
            relative,
            identity,
            entry_depth: entry_depth + 1,
        });
        Ok(())
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

impl Iterator for LocalDirectoryWalker {
    type Item = LocalResult<LocalDirectoryEntry>;

    /// Produces the next entry, opening at most one new directory as needed.
    fn next(&mut self) -> Option<Self::Item> {
        if self.terminated {
            return None;
        }
        if let Some(rooted) = self.rooted.as_mut() {
            let result = next_rooted_entry(rooted, self.options);
            if matches!(&result, Some(Err(_)))
                && self.options.error_policy() == LocalWalkErrorPolicy::FailFast
            {
                self.terminated = true;
            }
            return result;
        }
        loop {
            let frame = self.stack.last_mut()?;
            let entry_depth = frame.entry_depth;
            let relative_parent = frame.relative.clone();
            if frame.entries.is_none() {
                let directory = self.root.join(&relative_parent);
                if self.symlink_policy.follows() {
                    let identity = match fs::canonicalize(&directory) {
                        Ok(identity) => identity,
                        Err(error) => {
                            return Some(Err(walk_io_error(&directory, error)));
                        }
                    };
                    if frame.identity.as_ref() != Some(&identity) {
                        return Some(Err(
                            LocalFileError::new(
                                LocalFileErrorKind::InvalidPath,
                                LocalFileOperation::List,
                            )
                            .with_reason(
                                "directory identity changed while reopening walker frame",
                            )
                            .with_path(directory),
                        ));
                    }
                } else {
                    match fs::symlink_metadata(&directory) {
                        Ok(metadata) if metadata.file_type().is_dir() => {}
                        Ok(_) => {
                            return Some(Err(
                                LocalFileError::new(
                                    LocalFileErrorKind::InvalidPath,
                                    LocalFileOperation::List,
                                )
                                .with_reason(
                                    "directory entry changed while reopening walker frame",
                                )
                                .with_path(directory),
                            ));
                        }
                        Err(error) => {
                            return Some(Err(walk_io_error(&directory, error)));
                        }
                    }
                }
                match fs::read_dir(&directory) {
                    Ok(entries) => frame.entries = Some(entries),
                    Err(error) => {
                        if self.options.error_policy()
                            == LocalWalkErrorPolicy::FailFast
                        {
                            self.terminated = true;
                        }
                        return Some(Err(walk_io_error(&directory, error)));
                    }
                }
            }
            let next_entry = loop {
                let next = frame
                    .entries
                    .as_mut()
                    .expect("host walker reader was initialized")
                    .next();
                match next {
                    Some(Ok(entry)) if frame.seen.insert(entry.file_name()) => {
                        break Some(Ok(entry));
                    }
                    Some(Ok(_)) => continue,
                    other => break other,
                }
            };
            #[cfg(coverage)]
            let next_entry =
                if crate::local::take_coverage_fault("walker-entry") {
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
                        self.stack.pop().expect("stack is non-empty");
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
                && let Err(error) =
                    self.descend(&path, relative.clone(), entry_depth)
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

/// Produces the next descriptor-relative rooted entry.
///
/// # Parameters
///
/// - `state`: Rooted traversal state.
/// - `options`: Fixed traversal policy.
///
/// # Returns
///
/// The next structured entry or path-specific error, or `None` at completion.
fn next_rooted_entry(
    state: &mut RootedWalkState,
    options: LocalListOptions,
) -> Option<LocalResult<LocalDirectoryEntry>> {
    loop {
        let frame = state.stack.last_mut()?;
        let entry_depth = frame.entry_depth;
        let authority_parent = frame.authority_parent.clone();
        let output_parent = frame.output_parent.clone();
        if frame.reader.is_none() {
            let reader = if authority_parent.as_os_str().is_empty() {
                state.root.open_root_dir_reader()
            } else {
                let relative = match crate::local::LocalRelativePath::new(
                    &authority_parent,
                ) {
                    Ok(relative) => relative,
                    Err(error) => {
                        return Some(Err(walk_io_error(
                            &authority_parent,
                            error,
                        )));
                    }
                };
                state.root.open_dir_reader(&relative)
            };
            match reader {
                Ok(reader) => frame.reader = Some(reader),
                Err(error) => {
                    state.stack.pop();
                    return Some(Err(walk_io_error(&authority_parent, error)));
                }
            }
        }
        let next_entry = frame
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
                if let Some(frame) = state.stack.pop()
                    && let Some(identity) = frame.identity
                {
                    state.followed_directories.remove(&identity);
                }
                continue;
            }
            Err(error) => {
                state.stack.pop();
                return Some(Err(walk_io_error(&authority_parent, error)));
            }
        };
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
            let authority_root = state.root.authority_path();
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
                let identity = target.clone();
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
        let may_descend = if options.recursive() {
            match options.max_depth() {
                Some(max_depth) => entry_depth < max_depth,
                None => true,
            }
        } else {
            false
        };
        if is_directory && may_descend {
            if state.stack.len() >= options.max_open_directories()
                && options.reopen_policy()
                    == crate::LocalDirectoryReopenPolicy::Fail
            {
                return Some(Err(LocalFileError::new(
                    LocalFileErrorKind::ResourceLimit,
                    LocalFileOperation::List,
                )
                .with_path(state.root.path().join(&authority_path))));
            }
            if state.stack.len() >= options.max_open_directories() {
                for frame in &mut state.stack {
                    frame.reader = None;
                }
            }
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
