# RLF3 Windows enumeration fallback evidence

## Current contract and evidence status

The current Windows Rooted enumeration contract opens an independent directory handle when available. If an independent reopen is unavailable, enumeration returns `Unsupported`; it does not clone a handle that may share a cursor or perform the old six-open fallback. The historical six-fallback fixture and its cursor-independence assertions are obsolete for the current implementation. Historical commits, commands, logs, and results below are retained as evidence of their recorded source revisions only; they are not current acceptance criteria. Native execution evidence for the current fail-closed contract remains unknown unless obtained on Windows.

The current native Windows commands are:

```text
cargo test --locked --no-default-features --lib local::internal::windows_rooted::directory::fallback_tests::test_root_enumeration_fallback_fails_closed_without_independent_handle -- --exact --nocapture
cargo test --locked --all-features --lib local::internal::windows_rooted::directory::fallback_tests::test_root_enumeration_fallback_fails_closed_without_independent_handle -- --exact --nocapture
```

Both commands must show the exact test executing on native Windows to provide execution evidence. Linux filters and cross-compilation do not provide that evidence.

### Current native Windows run (2026-11-03)

- Source/test checkout HEAD: `daa0e0282a575885c904182cc1c456ba2e28329f`.
- OS: Microsoft Windows Server 2025, version `10.0.26100` Datacenter; runner image `windows-2025-vs2026`, version `20261030.294.1`.
- Filesystem/volume type: unknown; the job did not record it. Runner account permissions relevant to fixture creation: unknown; the job did not inspect them.
- Toolchain: `1.94.0-x86_64-pc-windows-msvc`, `rustc 1.94.0 (4a4ef493e 2026-03-02)`.
- `--no-default-features`: the exact test above reported `... ok`; `1 passed; 0 failed; 0 ignored; 0 measured; 413 filtered out`.
- `--all-features`: the exact test above reported `... ok`; `1 passed; 0 failed; 0 ignored; 0 measured; 545 filtered out`.
- Full runner output: [workflow run 41692830948, Windows rooted enumeration fallback job](https://github.com/qubit-ltd/rs-local-files/actions/runs/41692830948/jobs/126115550000).

## Historical evidence (prior implementation and source revisions)

### Historical RLF7-Q01 review (2026-11-03)

`BLOCKED_NATIVE`: a native Windows CI run exists for the exact source HEAD, but it does not contain a result for the RLF7-Q01 test. The Windows job failed while compiling unrelated code, before any Cargo test command ran. This is a verified Windows build failure, not evidence that the fallback test passed or failed. No production fallback change is supported by this run.

### Matching CI run and environment

- Source repository and HEAD: `qubit-ltd/rs-local-files`, `d4235f2dcfa34cf09ea1bf49ba4fe70bb4307c89`.
- Run inventory: `gh run list --repo qubit-ltd/rs-local-files --commit d4235f2dcfa34cf09ea1bf49ba4fe70bb4307c89 --json databaseId,headSha,workflowName,event,status,conclusion,url,createdAt --limit 100` returned exactly one run: Rust CI run `41543257374`, push to `main`, created `2026-11-02T23:10:03Z`, conclusion `failure`.
- Windows job: `rust-ci / verify (windows-latest)`, job ID `125612413682`; [run](https://github.com/qubit-ltd/rs-local-files/actions/runs/41543257374), [job log](https://github.com/qubit-ltd/rs-local-files/actions/runs/41543257374/job/125612413682). The run log identifies Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, runner image `windows-2025-vs2026` version `20261030.294.1`.
- Toolchain: the job installed `1.94-x86_64-pc-windows-msvc`, rustc `1.94.1 (e408947bf 2026-03-25)`; it also reports project build toolchain `1.94.0-x86_64-pc-windows-msvc`, rustc `1.94.0 (4a4ef493e 2026-03-02)`.
- The CI log does not record the filesystem/volume type or runner account's filesystem permission details. These facts are therefore unavailable; do not infer them from the hosted runner label.
- Current review host (not the CI host): `Linux mas-server 6.8.0-147-generic #147-Ubuntu SMP PREEMPT_DYNAMIC Thu Sep 24 11:33:16 UTC 2026 x86_64`; filesystem `ext4`; uid/gid `1001(starfish):1001(developer)`. Local toolchain: rustc/cargo `1.94.0`, active `1.94.0-x86_64-unknown-linux-gnu`.

### What the matching Windows log establishes

The reusable verify job stopped in `cargo build --locked --workspace --all-targets --all-features`. The complete compiler diagnostics identify two `E0308` mismatches at `src\\local\\internal\\conditional\\windows\\namespace.rs:188`: both tuple values returned as `String` where `OsString` was expected. Cargo ended with `could not compile qubit-local-files (lib) due to 2 previous errors`; the orchestration log reports `run --suite build failed`. The test stage was never reached. This is an unrelated Windows compilation blocker; it does not establish the fallback behavior either way.

The desired target is `local::internal::windows_rooted::directory::fallback_tests::test_root_enumeration_fallback_keeps_independent_cursors` (library unit test). The fixture source requires six forced fallback opens and checks sequential drains, alternating readers, a dropped partially advanced reader, and complete unique fixture name sets. No output from this test, execution count, fixture result, or `fallback_count == 6` appears in the matching run.

### Missing evidence and required native commands

The missing evidence is successful execution of that exact test once in both default and all-features configurations on Windows at the RLF7 source revision, including successful fixture creation, complete unique name sets, and `fallback_count == 6`. The available Windows run's build failure prevents both commands below from running; Linux cannot substitute for native Windows filesystem and handle semantics:

```text
cargo test --locked --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
cargo test --locked --all-features --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
```

First resolve the Windows compilation blocker. Then run both commands on a native Windows checkout at a source revision that contains the Q01 fixture and builds successfully on Windows; record the actual source HEAD used. Keep this run's `d4235f2dcfa34cf09ea1bf49ba4fe70bb4307c89` status explicit: its Windows job failed before the target test ran and provides no Q01 result. Record the OS, filesystem/volume, toolchain, features, account permissions relevant to fixture creation, full test name and execution count, and logs/URLs for the subsequent runs. Preserve `BLOCKED_NATIVE` until both runs provide the required evidence. No production code was changed as part of this review.

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

#### Historical evidence retained

The earlier inspection recorded target HEAD `50e168b4853b777752a75f31a77900bbb6cd476d` and two Linux filter runs that exited 0 but reported `running 0 tests` (474 and 605 filtered out). Those results are historical, do not match the current source HEAD, and are not passes. They establish neither independent native enumeration cursors nor Windows behavior. The Windows targets installed at that time likewise did not replace native execution.

That inspection recorded the original repository path as `/data/working/qubit/rust-common/rs-local-files`, Linux host filesystem `ext4`, uid/gid 1000 (`starfish`), Rust `1.94.0 (4a4ef493e 2026-03-02)` with active toolchain `1.94.0-x86_64-unknown-linux-gnu`, and installed targets `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc`. It also recorded dependency source commits rs-redact `04f4bef3ac9a391cc220438918dcb1451a1963a6` and rs-json `0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab`. The two historical commands were the default `cargo test --locked --lib ...` and the same filter with `--all-features --lib`; both exited 0 but executed zero matching tests. They were run from a clean integration worktree based on that historical target HEAD, with the two path dependencies resolving to clean sibling repositories.

The inspected regression fixture in `src/local/internal/windows_rooted/directory/fallback_tests.rs` creates 700 long names beyond the estimated 64 KiB directory-record limit and exercises sequential reads, alternating reads, and dropping a partially advanced reader. The test is Windows-gated; source inspection alone does not execute it.

The original evidence's requested next step was to run both exact filters on native Windows and record OS version, filesystem, Rust toolchain, target HEAD, features, permissions, full test name, and actual test count, verifying six forced fallbacks. It explicitly withheld the then-planned T5 decision (independent-handle reopen or `Unsupported`) absent a native reproduction; fixture or permission failures were to be distinguished from cursor interference.

## Decision

Q01 remains `BLOCKED_NATIVE` / unverified. No matching local Windows logs or native Windows execution are available in this review environment. No production fallback code was changed. Revisit only with current-HEAD Windows execution logs that identify OS, filesystem, Rust/toolchain and target, features, permissions, full test name, actual count, and outcome; distinguish fixture or permission failures from cursor interference before considering a production change.
