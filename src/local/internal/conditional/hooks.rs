// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Compatibility module preserving the historical conditional hook path.

#[path = "conditional_hooks.rs"]
mod conditional_hooks;

pub(crate) use conditional_hooks::ConditionalHooks;
