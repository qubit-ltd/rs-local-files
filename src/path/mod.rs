// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Scope-bound native path and filename values.

mod local_file_names;
mod local_path_codec;
mod local_paths;
mod relative_path;

#[cfg(test)]
mod path_tests;

pub use local_file_names::LocalFileNames;
pub use local_path_codec::LocalPathCodec;
pub use local_paths::LocalPaths;
pub(crate) use relative_path::RelativePath;
