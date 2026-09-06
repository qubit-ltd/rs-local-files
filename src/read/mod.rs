// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native local file read operations.

mod open_options;
#[cfg(any(windows, test))]
mod vectored;

pub use open_options::OpenOptions;
#[cfg(any(windows, test))]
pub(crate) use vectored::read_vectored_fallback;
