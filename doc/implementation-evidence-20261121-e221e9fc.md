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

References: [Rust `Vec::try_reserve_exact`](https://doc.rust-lang.org/alloc/vec/struct.Vec.html#method.try_reserve_exact), [Rust `RawVec` source](https://doc.rust-lang.org/src/alloc/raw_vec/mod.rs.html).
