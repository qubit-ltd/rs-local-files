// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Compile tests for the exact bilingual README and user-guide examples.
//!
//! Keeping the source Markdown attached directly to this module avoids a
//! second copied fixture that could drift independently from the published
//! documentation.
#![doc = include_str!("../README.md")]
#![doc = include_str!("../README.zh_CN.md")]
#![doc = include_str!("../doc/user_guide.md")]
#![doc = include_str!("../doc/user_guide.zh_CN.md")]
