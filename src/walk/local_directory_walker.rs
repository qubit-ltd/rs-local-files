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
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    LocalDirectoryEntry, LocalFileError, LocalFileErrorKind, LocalFileMetadata, LocalFileOperation,
    LocalListOptions, LocalResult,
};

use super::internal::{RootedWalkFrame, RootedWalkState, WalkFrame};

/// Lazy depth-first iterator over native local directory entries.
#[derive(Debug)]
pub struct LocalDirectoryWalker {
    /// Bound traversal root.
    root: PathBuf,
    /// Policy fixed when the walker is created.
    options: LocalListOptions,
    /// Open directory iterators, bounded by traversal depth.
    stack: Vec<WalkFrame>,
    /// Canonical directory identities used only for follow-mode cycle
    /// detection.
    followed_directories: HashSet<PathBuf>,
    /// Descriptor-relative traversal state for a rooted walker.
    rooted: Option<RootedWalkState>,
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
    pub(crate) fn open(root: PathBuf, options: LocalListOptions) -> LocalResult<Self> {
        let metadata = fs::symlink_metadata(&root).map_err(|error| walk_io_error(&root, error))?;
        if !metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::List,
            )
            .with_path(root));
        }
        let entries = fs::read_dir(&root).map_err(|error| walk_io_error(&root, error))?;
        let mut followed_directories = HashSet::new();
        if options.follows_symlinks() {
            let identity = fs::canonicalize(&root).map_err(|error| walk_io_error(&root, error))?;
            followed_directories.insert(identity);
        }
        Ok(Self {
            root,
            options,
            stack: vec![WalkFrame {
                entries,
                relative: PathBuf::new(),
                entry_depth: 1,
            }],
            followed_directories,
            rooted: None,
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
    /// Returns `LocalFileError` when follow mode is requested, or the initial
    /// rooted directory cannot be read.
    pub(crate) fn open_rooted(
        root: Arc<crate::rooted::Root>,
        path: Option<crate::local::LocalRelativePath>,
        options: LocalListOptions,
    ) -> LocalResult<Self> {
        if options.follows_symlinks() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::List,
            ));
        }
        let authority_parent = path
            .as_ref()
            .map_or_else(PathBuf::new, |path| path.as_path().to_path_buf());
        let entries = match path.as_ref() {
            Some(path) => root.read_dir(path),
            None => root.read_root_dir(),
        }
        .map_err(|error| walk_io_error(&authority_parent, error))?;
        let diagnostic_root = root.path().join(&authority_parent);
        Ok(Self {
            root: diagnostic_root,
            options,
            stack: Vec::new(),
            followed_directories: HashSet::new(),
            rooted: Some(RootedWalkState {
                root,
                stack: vec![RootedWalkFrame {
                    entries: entries.into_iter(),
                    authority_parent,
                    output_parent: PathBuf::new(),
                    entry_depth: 1,
                }],
            }),
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
    fn descend(&mut self, path: &Path, relative: PathBuf, entry_depth: usize) -> LocalResult<()> {
        if self.options.follows_symlinks() {
            let identity = fs::canonicalize(path).map_err(|error| walk_io_error(path, error))?;
            if !self.followed_directories.insert(identity) {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::InvalidInput,
                    LocalFileOperation::List,
                )
                .with_path(path.to_path_buf()));
            }
        }
        let entries = fs::read_dir(path).map_err(|error| walk_io_error(path, error))?;
        self.stack.push(WalkFrame {
            entries,
            relative,
            entry_depth: entry_depth + 1,
        });
        Ok(())
    }
}

impl Iterator for LocalDirectoryWalker {
    type Item = LocalResult<LocalDirectoryEntry>;

    /// Produces the next entry, opening at most one new directory as needed.
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(rooted) = self.rooted.as_mut() {
            return next_rooted_entry(rooted, self.options);
        }
        loop {
            let frame = self.stack.last_mut()?;
            let entry_depth = frame.entry_depth;
            let relative_parent = frame.relative.clone();
            let entry = match frame.entries.next() {
                Some(Ok(entry)) => entry,
                Some(Err(error)) => {
                    return Some(Err(walk_io_error(&self.root.join(&relative_parent), error)));
                }
                None => {
                    self.stack.pop();
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
            let native_metadata = if self.options.follows_symlinks() {
                fs::metadata(&path)
            } else {
                fs::symlink_metadata(&path)
            };
            let native_metadata = match native_metadata {
                Ok(metadata) => metadata,
                Err(error) => return Some(Err(walk_io_error(&path, error))),
            };
            let is_directory = native_metadata.file_type().is_dir();
            let metadata = LocalFileMetadata::from_native(&native_metadata);

            if is_directory
                && self.may_descend(entry_depth)
                && let Err(error) = self.descend(&path, relative.clone(), entry_depth)
            {
                return Some(Err(error));
            }
            return Some(Ok(LocalDirectoryEntry::new(path, relative, metadata)));
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
        let entry = match frame.entries.next() {
            Some(entry) => entry,
            None => {
                state.stack.pop();
                continue;
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
        let metadata = crate::rooted_local_file_system::rooted_metadata(entry.metadata());
        let is_directory = metadata.kind() == crate::LocalFileKind::Directory;
        let may_descend = options.recursive()
            && options
                .max_depth()
                .is_none_or(|max_depth| entry_depth < max_depth);
        if is_directory && may_descend {
            let relative = match crate::local::LocalRelativePath::new(&authority_path) {
                Ok(relative) => relative,
                Err(error) => {
                    return Some(Err(walk_io_error(&authority_path, error)));
                }
            };
            let entries = match state.root.read_dir(&relative) {
                Ok(entries) => entries,
                Err(error) => {
                    return Some(Err(walk_io_error(&authority_path, error)));
                }
            };
            state.stack.push(RootedWalkFrame {
                entries: entries.into_iter(),
                authority_parent: authority_path.clone(),
                output_parent: output_path.clone(),
                entry_depth: entry_depth + 1,
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
#[inline(always)]
fn walk_io_error(path: &Path, error: std::io::Error) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::List,
        Some(path.to_path_buf()),
        None,
        error,
    )
}
