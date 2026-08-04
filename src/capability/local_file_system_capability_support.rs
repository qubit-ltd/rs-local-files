// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0 (the "License");
//    you may not use this file except in compliance with the License.
//    You may obtain a copy of the License at
//
//        http://www.apache.org/licenses/LICENSE-2.0
//
//    Unless required by applicable law or agreed to in writing, software
//    distributed under the License is distributed on an "AS IS" BASIS,
//    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//    See the License for the specific language governing permissions and
//    limitations under the License.
// =============================================================================
//! Support levels for native filesystem capability snapshots.

/// Describes how strongly a local filesystem capability is known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalFileSystemCapabilitySupport {
    /// The implementation is available, but the active mount was not probed.
    Implemented,
    /// The active authority or mount was explicitly verified at runtime.
    RuntimeVerified,
    /// The implementation cannot make a reliable support claim.
    Unknown,
}
