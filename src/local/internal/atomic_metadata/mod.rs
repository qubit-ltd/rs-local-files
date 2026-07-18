// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Strict platform-native metadata preservation for atomic replacement.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

#[cfg(target_os = "freebsd")]
mod freebsd;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux_android;
#[cfg(target_os = "macos")]
mod macos;
mod preservation;

pub(crate) use preservation::preserve_atomic_metadata;
