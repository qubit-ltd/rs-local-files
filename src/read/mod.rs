// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local file read operations.

mod open;
mod open_options;

pub use open::{
    open,
    open_with,
};
pub use open_options::OpenOptions;
