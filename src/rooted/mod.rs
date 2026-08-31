// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative local filesystem operations.

mod copy;
mod directory_reader;
mod entry;
mod entry_kind;
mod metadata;
mod path;
mod permissions;
mod root;
mod work;

pub use directory_reader::DirectoryReader;
pub(crate) use entry::Entry;
pub use entry_kind::EntryKind;
pub use metadata::Metadata;
pub use path::Path;
pub use permissions::Permissions;
pub use root::Root;

pub(crate) use crate::local::LocalRootAtomicWriter as Writer;
