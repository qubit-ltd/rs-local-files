// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative local filesystem operations.

mod metadata;
mod path;
mod root;

pub use metadata::{
    EntryKind,
    Metadata,
};
pub use path::Path;
pub use root::Root;
