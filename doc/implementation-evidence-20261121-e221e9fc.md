# RLF-053 capacity reachability evidence

## Environment and probe

- Locked Rust toolchain: `1.94.0` (`rustc 1.94.0 (4a4ef493e 2026-03-02)`), host `x86_64-unknown-linux-gnu`, LLVM `21.1.8`.
- Supported platform configuration: Linux, Windows, and macOS (`.infra/platform/platform.toml`).
- Allocator: the toolchain/runtime default global allocator on Linux; the repository does not declare a custom `#[global_allocator]`.
- Probe: a standalone program constructed an empty `Vec<u8>`, called `try_reserve_exact(n)`, and recorded `capacity()` for `n = 1, 2, 3, 7, 15, 16, 17, 63, 64, 65, 255, 256, 257, 4095, 4096, 4097, 65535, 65536, 65537`.
- Result: every probe reported `capacity() == n`.
- Command: `rustc --version --verbose`; then `rustc -o /tmp/rlf053_vec_capacity_probe - <<'RS' ... RS` and `/tmp/rlf053_vec_capacity_probe`.

Rust documents that `try_reserve_exact` may allocate more than requested, while the current `Global` implementation returns the requested capacity unless the allocator reports a larger usable size. The probe covers only Linux with the default allocator; Windows, macOS, and custom allocators were not exercised. Thus production reachability of the over-allocation branch is **not established**, and no production policy change is justified by this evidence.

## Synthetic branch validation

The byte-permit reconciliation was factored into a small internal helper so the allocator result can be supplied deterministically. Two tests exercise synthetic `actual_delta > requested_delta`: one confirms the permit grows to cover the actual capacity, and one confirms the allocation is rejected and the permit fully refunded when the configured pool cannot cover the excess. This validates the accounting branch independently from whether the current production allocator reaches it.

The production path still measures `Vec::capacity()` after `try_reserve_exact`; future platform or allocator evidence can determine whether further policy changes are needed.

## Follow-up verification (2026-11-22)

The Linux probe was rerun with the locked Rust 1.94.0 toolchain on
`x86_64-unknown-linux-gnu`. For each listed request from 1 through 65,537
bytes, `try_reserve_exact` again returned exactly the requested capacity. The
synthetic reconciliation tests now also cover actual capacity below and equal
to the requested amount, plus an actual capacity of seven bytes crossing a
synthetic four-byte per-operation ceiling while the seven-byte shared pool can
still account for it. A separate existing test covers the same over-allocation
when the shared pool cannot fund it and verifies full refund. These synthetic
cases characterize accounting only; they do not establish allocator
reachability or change the production policy.

The host has the Apple target libraries installed, but the follow-up Apple
probe could not be linked: `xcrun`/the macOS SDK is absent and the available GNU
linker rejects Apple's `-arch` and deployment-target flags. Windows and custom
allocator runtime probes were unavailable. Therefore the only supported
runtime evidence remains Linux with its default allocator; no cross-platform
reachability conclusion is claimed.

References: [Rust `Vec::try_reserve_exact`](https://doc.rust-lang.org/alloc/vec/struct.Vec.html#method.try_reserve_exact), [Rust `RawVec` source](https://doc.rust-lang.org/src/alloc/raw_vec/mod.rs.html).

## RLF-053 implementation-round check (2026-11-22)

The probe was rerun with `rustc 1.94.0 (4a4ef493e 2026-03-02)` on
`x86_64-unknown-linux-gnu`, using the process default global allocator. The
command compiled and ran a standalone `Vec<u8>` program that called
`try_reserve_exact` for each request listed above. All 19 observed capacities
equaled their requested sizes. Windows, macOS, and custom allocator runtime
probes remain unavailable in this environment, so this result still does not
establish production reachability of allocator over-allocation. No production
capacity policy change is warranted by the available evidence.

## RLF-053 implementation-plan execution (2026-11-22)

- Runtime: Linux `6.8.0-147-generic`, `x86_64-unknown-linux-gnu`.
- Toolchain: locked Rust `1.94.0 (4a4ef493e 2026-03-02)`, LLVM `21.1.8`.
- Allocator: process default global allocator; this crate declares no custom
  global allocator.
- Command: `cargo test --locked --features async --lib capacity` — 24 passed,
  0 failed. The synthetic over-allocation reconciliation tests pass, including
  the case where actual capacity crosses a per-operation ceiling while the
  shared pool can account for it; these tests do not establish runtime
  reachability.
- The earlier Linux probe in this document measured 19 `try_reserve_exact`
  requests and observed exact requested capacities. The current environment
  has Apple and Windows GNU Rust targets installed, but it cannot run either
  target; `xcrun` and `wine` are unavailable. No macOS, Windows, or custom
  allocator runtime evidence was obtained.

Conclusion: supported-runtime evidence still does not show that production
allocation can exceed the requested capacity. Keep the current capacity
settlement policy and diagnostics; revisit only if a supported runtime or
downstream allocator supplies a reproducible reachability case.

## RLF-053 round 4 reachability check (2026-11-22)

- Runtime: Linux `6.8.0-147-generic`, `x86_64-unknown-linux-gnu`.
- Toolchain: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, LLVM `21.1.8`.
- Allocator: process default global allocator; this crate declares no custom
  global allocator.
- Command: `rustc -o /tmp/rlf053-round4-vec-capacity-probe - <<'RS' ... RS`,
  followed by `/tmp/rlf053-round4-vec-capacity-probe`. The probe called
  `try_reserve_exact` for the same 19 requests listed above. Every reported
  capacity equaled its requested size.
- `xcrun` and `wine` are unavailable. Installed Apple and Windows GNU Rust
  targets cannot provide runtime allocator evidence on this Linux host.

Conclusion: no supported runtime in this round demonstrated allocator
over-allocation. Keep the existing production capacity settlement policy;
synthetic accounting tests remain useful but do not establish reachability.

## RLF-053 round 7 environment check (2026-11-22)

- `rustup target list --installed` includes `x86_64-apple-darwin` and
  `x86_64-pc-windows-gnu`, but these are cross-compilation targets, not
  runnable runtimes on this Linux host.
- `command -v xcrun` and `command -v wine` found no executable, so a macOS SDK
  and Windows runtime are unavailable for allocator probes.
- The crate has no production `#[global_allocator]`; the only allocator
  implementation found by the repository search is the sampling allocator in
  `benches/local_files.rs`, which is not linked into the library or its tests.

No different supported runtime or downstream custom allocator was available
for this round. The Linux default-allocator probe and synthetic accounting
tests above remain the applicable evidence; no production policy change is
indicated.
