// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative filesystem primitives.

mod directory_cursor;
mod directory_entry;
mod entry_identity;
mod filesystem_probe;
mod namespace_handle;
mod opened_file;
mod staged_file;

pub(crate) use directory_cursor::DirectoryCursor;
pub(crate) use directory_entry::PlatformDirectoryEntry;
pub(crate) use entry_identity::EntryIdentity;
pub(crate) use namespace_handle::NamespaceHandle;
pub(crate) use opened_file::OpenedFile;
pub(crate) use staged_file::StagedFile;
pub(crate) use staged_file::StagedInstallError;
pub(crate) use staged_file::StagedInstallState;
