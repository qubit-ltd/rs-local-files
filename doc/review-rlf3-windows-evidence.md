# RLF3 Windows enumeration fallback evidence

## Current RLF6 review (2026-11-02)

`BLOCKED_NATIVE`: RLF6-Q01 remains unverified. This review found no locally accessible Windows runner log for the current source HEAD, and this environment is Linux. The existing Windows-gated test therefore has not been shown to pass or reproduce a defect. No production fallback change is supported, and Q01 is not closed.

### Source and environment

- Source repository/worktree: `/data/working/qubit/rust-common/rs-local-files-rlf6`
- Current source HEAD: `c03588f306b6c8325ecb991b38197e655eb135b7` (`docs: document Windows privacy contracts`, 2026-11-02)
- Host: `Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026 x86_64 GNU/Linux`
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `x86_64-unknown-linux-gnu`; active toolchain `1.94.0-x86_64-unknown-linux-gnu`
- Installed targets: `x86_64-pc-windows-gnu`, `x86_64-unknown-linux-gnu`. A Windows compilation target does not provide a native Windows runtime.
- No Windows host, Windows filesystem, or Windows permission context is available here.
- Local search covered the repository and `/data/working/qubit/rust-common`, `/tmp`, and `/home/starfish` for Windows runner/log artifacts and the test name. No corresponding current-HEAD Windows execution log was found. No external service was queried.

### Current commands and results

Attempted from the worktree root:

```text
cargo test --locked --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
```

The Linux build did not complete: Cargo reported concurrent missing files under `target/debug` while writing fingerprints/linking dependencies (`No such file or directory`); no test binary ran, so there is no test count or test result. This failed build is not Windows evidence. The all-features command was not run because the shared build artifacts were concurrently unavailable; neither command can provide a native Windows result on this host.

The native commands still required are:

```text
cargo test --locked --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
cargo test --locked --all-features --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
```

They must run on a native Windows runner at a recorded source HEAD. A qualifying result must show the named test actually executed once in each configuration and cover the fixture's sequential drains, alternating readers, dropped partially advanced reader, unique complete name sets, and `fallback_count == 6`.

### Historical evidence retained

The earlier inspection recorded target HEAD `50e168b4853b777752a75f31a77900bbb6cd476d` and two Linux filter runs that exited 0 but reported `running 0 tests` (474 and 605 filtered out). Those results are historical, do not match the current source HEAD, and are not passes. They establish neither independent native enumeration cursors nor Windows behavior. The Windows targets installed at that time likewise did not replace native execution.

That inspection recorded the original repository path as `/data/working/qubit/rust-common/rs-local-files`, Linux host filesystem `ext4`, uid/gid 1000 (`starfish`), Rust `1.94.0 (4a4ef493e 2026-03-02)` with active toolchain `1.94.0-x86_64-unknown-linux-gnu`, and installed targets `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc`. It also recorded dependency source commits rs-redact `04f4bef3ac9a391cc220438918dcb1451a1963a6` and rs-json `0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab`. The two historical commands were the default `cargo test --locked --lib ...` and the same filter with `--all-features --lib`; both exited 0 but executed zero matching tests. They were run from a clean integration worktree based on that historical target HEAD, with the two path dependencies resolving to clean sibling repositories.

The inspected regression fixture in `src/local/internal/windows_rooted/directory/fallback_tests.rs` creates 700 long names beyond the estimated 64 KiB directory-record limit and exercises sequential reads, alternating reads, and dropping a partially advanced reader. The test is Windows-gated; source inspection alone does not execute it.

The original evidence's requested next step was to run both exact filters on native Windows and record OS version, filesystem, Rust toolchain, target HEAD, features, permissions, full test name, and actual test count, verifying six forced fallbacks. It explicitly withheld the then-planned T5 decision (independent-handle reopen or `Unsupported`) absent a native reproduction; fixture or permission failures were to be distinguished from cursor interference.

## Decision

Q01 remains `BLOCKED_NATIVE` / unverified. No matching local Windows logs or native Windows execution are available in this review environment. No production fallback code was changed. Revisit only with current-HEAD Windows execution logs that identify OS, filesystem, Rust/toolchain and target, features, permissions, full test name, actual count, and outcome; distinguish fixture or permission failures from cursor interference before considering a production change.
