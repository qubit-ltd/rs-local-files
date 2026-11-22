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
