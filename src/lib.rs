// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! # Qubit Local Files
//!
//! Unified native local filesystem operations for Rust.
//!
//! [`LocalFileSystem`] provides host-wide operations. [`RootedLocalFileSystem`]
//! anchors descendant operations to an opened directory descriptor or handle.
//! [`LocalFileNames`] and [`LocalPaths`] provide native lexical utilities,
//! while readers, writers, walkers, and temporary resources retain explicit
//! ownership and lifecycle state.
//!
//! Scattered legacy free-function namespaces are intentionally unavailable:
//!
//! ```compile_fail
//! use qubit_local_files::directory;
//! ```
//!
//! Rooted authority is exposed only through the unified stateful type:
//!
//! ```compile_fail
//! use qubit_local_files::rooted::Root;
//! ```

#[cfg(coverage)]
pub mod atomic;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod atomic;
mod capability;
#[cfg(coverage)]
pub mod copy;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod copy;
#[cfg(coverage)]
pub mod directory;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod directory;
mod error;
#[cfg(coverage)]
pub mod local;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod local;
mod local_file_kind;
mod local_file_metadata;
mod local_file_names;
mod local_file_reader;
mod local_file_system;
mod local_paths;
#[cfg(coverage)]
pub mod metadata;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod metadata;
mod options;
mod outcome;
#[cfg(coverage)]
pub mod path;
#[cfg(coverage)]
pub mod read;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod read;
#[cfg(coverage)]
pub mod remove;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod remove;
#[cfg(coverage)]
pub mod rename;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod rename;
#[cfg(coverage)]
pub mod rooted;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod rooted;
mod rooted_local_file_system;
#[cfg(coverage)]
pub mod temp;
mod walk;
#[cfg(coverage)]
pub mod write;
#[cfg(not(coverage))]
#[allow(dead_code, unused_imports)]
mod write;
mod writer;

/// Native primitives for filesystem-provider implementations.
///
/// Application code should use [`LocalFileSystem`] or
/// [`RootedLocalFileSystem`] instead. This namespace groups the low-level
/// operations needed by provider adapters without restoring their former
/// crate-root module paths.
pub mod backend {
    /// Atomic replacement primitives for provider adapters.
    pub mod atomic {
        pub use crate::atomic::{
            CommitError,
            DestinationState,
            Error,
            Options,
            Stage,
            Writer,
            begin,
            begin_with,
            write,
            write_with,
        };
    }

    /// Recursive copy primitives for provider adapters.
    pub mod copy {
        pub use crate::copy::{
            ConflictPolicy,
            Error,
            Options,
            Stage,
            Statistics,
            TypeConflictPolicy,
            directory,
            file,
            file_without_replacing,
        };
    }

    /// Directory primitives for provider adapters.
    pub mod directory {
        pub use crate::directory::{
            clear,
            create,
            create_all,
            create_parent,
            read,
            size,
        };
    }

    /// Native reader primitives for provider adapters.
    pub mod read {
        pub use crate::read::{
            OpenOptions,
            open,
            open_with,
        };
    }

    /// Removal primitives for provider adapters.
    pub mod remove {
        pub use crate::remove::{
            any,
            directory_tree,
            empty_directory,
            file,
        };
    }

    /// Rename primitives for provider adapters.
    pub mod rename {
        pub use crate::rename::{
            move_path,
            move_path_without_replacing,
        };
    }

    /// Descriptor-relative primitives for provider adapters.
    pub mod rooted {
        pub use crate::rooted::{
            Entry,
            EntryKind,
            Metadata,
            Path,
            Permissions,
            Root,
            Writer,
        };
    }

    /// Native writer primitives for provider adapters.
    pub mod write {
        pub use crate::write::{
            Mode,
            OpenOptions,
            open,
        };
    }
}

pub use capability::{
    LocalFileSystemCapabilities,
    LocalPathLengthUnit,
    LocalPathLimit,
};
pub use error::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalMutationState,
    LocalResult,
};
pub use local::{
    LocalCopyConflictPolicy,
    LocalCopyTypeConflictPolicy,
    LocalPersistError,
    LocalPersistOptions,
    LocalPersistStage,
    LocalTempDir as LocalTempDirectory,
    LocalTempFile,
};
pub use local_file_kind::LocalFileKind;
pub use local_file_metadata::LocalFileMetadata;
pub use local_file_names::LocalFileNames;
pub use local_file_reader::LocalFileReader;
pub use local_file_system::LocalFileSystem;
pub use local_paths::LocalPaths;
pub use options::{
    LocalAtomicityRequirement,
    LocalCopyOptions,
    LocalCreateDirectoryOptions,
    LocalCrossDevicePolicy,
    LocalDeleteOptions,
    LocalDurabilityRequirement,
    LocalListOptions,
    LocalMetadataPreservePolicy,
    LocalReadOptions,
    LocalRenameOptions,
    LocalSymlinkPolicy,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};
pub use outcome::{
    LocalCopyMethod,
    LocalCopyOutcome,
    LocalCopyStats,
    LocalCreateDirectoryOutcome,
    LocalDeleteOutcome,
    LocalRenameOutcome,
};
pub use rooted_local_file_system::RootedLocalFileSystem;
pub use walk::{
    LocalDirectoryEntry,
    LocalDirectoryWalker,
};
pub use writer::{
    LocalFileCommitError,
    LocalFileWriter,
    LocalWriteOutcome,
    LocalWriterState,
};

pub(crate) use local::{
    LocalAtomicCommitError,
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
    LocalCopyDirError,
    LocalCopyDirOptions,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalRelativePath,
};
