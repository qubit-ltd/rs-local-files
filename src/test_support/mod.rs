// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Deterministic fault injection for downstream adapter tests.
//!
//! This module is available only with the `test-support` feature. It is not a
//! production API and does not carry the crate's normal semver compatibility
//! promise. Instance-local plans validate isolated facade behavior; the
//! process-local fault guard serializes installation; ordinary selectors are
//! thread-affine, while explicitly shared selectors and named gates support
//! async adapters whose native work runs on worker threads. These helpers are
//! for native boundary and recovery-state tests that ordinary inputs cannot
//! trigger.

#[cfg(all(feature = "digest", feature = "test-support"))]
mod conditional_hooks;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod conditional_point;
#[cfg(feature = "test-support")]
mod host_metadata_probes;
mod internal;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod test_conditional_hooks;
#[cfg(all(feature = "digest", feature = "test-support"))]
mod test_conditional_point;
mod test_fault_plan;
mod test_fault_point;
#[cfg(feature = "test-support")]
mod test_gate;

#[cfg(all(feature = "digest", feature = "test-support"))]
#[doc(hidden)]
pub use conditional_hooks::TestConditionalHooks;
#[cfg(all(feature = "digest", feature = "test-support"))]
#[doc(hidden)]
pub use conditional_point::TestConditionalPoint;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use host_metadata_probes::host_metadata_probe_counts;
#[cfg(feature = "test-support")]
pub(crate) use host_metadata_probes::record_host_metadata_query;
#[cfg(feature = "test-support")]
pub(crate) use host_metadata_probes::record_host_prefix_probe;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use host_metadata_probes::reset_host_metadata_probe_counts;
#[cfg(feature = "test-support")]
pub(crate) use internal::temp_cleanup_add_child;
#[cfg(feature = "test-support")]
pub(crate) use internal::temp_cleanup_before_remove;
#[cfg(feature = "test-support")]
pub(crate) use internal::temp_cleanup_deadline_expired;
#[cfg(feature = "test-support")]
pub(crate) use internal::temp_cleanup_metadata;
#[cfg(all(any(unix, windows), feature = "test-support"))]
pub(crate) use internal::temp_cleanup_replace_observed_entry;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_fault_plan::TestFaultPlan;
pub use test_fault_point::TestFaultPoint;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_gate::TestGateGuard;
#[cfg(feature = "test-support")]
pub(crate) use test_gate::hit_test_gate_for_authority_path;
#[cfg(feature = "test-support")]
pub(crate) use test_gate::hit_test_gate_for_authority;
#[cfg(feature = "test-support")]
pub(crate) use test_gate::hit_test_gate_for_resource;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_gate::install_test_gate;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use test_gate::install_test_gate_for_path;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_authority_path;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_authority;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_filesystem;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_local_filesystem;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_local_filesystem_path;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_resource;
#[cfg(feature = "test-support")]
pub use test_gate::install_test_gate_for_filesystem_path;

#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::TestFaultGuard;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::install_test_fault;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::install_test_fault_shared;
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub use crate::local::install_test_fault_shared_for_path;
