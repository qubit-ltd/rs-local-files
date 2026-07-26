// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local path value types.

mod file_name;
mod portable_file_name;

pub use file_name::{
    DEFAULT_RANDOM_FILE_NAME_PREFIX,
    dot_extension,
    extension,
    file_name,
    file_name_from_path,
    file_name_from_url,
    file_prefix,
    file_stem,
    has_extension,
    has_extension_ignore_ascii_case,
    random_file_name,
    random_file_name_with,
    validate_portable_file_name,
};
pub use portable_file_name::PortableFileName;
