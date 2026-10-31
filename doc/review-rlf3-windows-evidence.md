# RLF3 Windows enumeration fallback evidence

## Status

`BLOCKED_NATIVE`: this review environment is Linux, so it cannot execute the Windows-only fallback test natively. No production fallback change is justified by the available evidence. RLF3-Q01 remains open and continues RLF2-Q03.

## Environment

- Target repository: `/data/working/qubit/rust-common/rs-local-files`
- Target HEAD at inspection: `50e168b4853b777752a75f31a77900bbb6cd476d`
- Host: `Linux starfish-personal-os 7.0.0-38-generic #24.04.4-Ubuntu SMP PREEMPT_DYNAMIC x86_64 GNU/Linux`
- Host filesystem for the worktree: ext4
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`; active toolchain `1.94.0-x86_64-unknown-linux-gnu`
- Installed Windows targets include `x86_64-pc-windows-gnu` and `x86_64-pc-windows-msvc`; a Windows target is not a native Windows runtime.
- Process identity: uid/gid 1000 (`starfish`). No Windows host, Windows filesystem, or Windows permission context is available.
- Dependency source commits checked: rs-redact `04f4bef3ac9a391cc220438918dcb1451a1963a6`; rs-json `0bc4aa9eb0aa2993aa4a75c4162e7ae45f2bebab`.

## Existing regression fixture

Source inspection of `src/local/internal/windows_rooted/directory/fallback_tests.rs` confirms that the existing regression creates 700 long names whose estimated directory record size exceeds 64 KiB. It checks two readers drained in sequence, two readers advanced in alternation, a partially advanced reader dropped before draining another, complete unique name sets, and `fallback_count == 6`.

The test is Windows-gated. On this Linux host the test body and fixture did not execute; this is not a pass and provides no evidence for independent native enumeration cursors.

## Commands and results

Commands were run from a clean integration worktree based on the target HEAD above, with the two path dependencies resolving to their existing clean sibling repositories:

```text
cargo test --locked --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
```

Exit code 0; `running 0 tests`; 474 tests filtered out. The named Windows-only test did not execute.

```text
cargo test --locked --all-features --lib test_root_enumeration_fallback_keeps_independent_cursors -- --nocapture
```

Exit code 0; `running 0 tests`; 605 tests filtered out. The named Windows-only test did not execute.

The installed Windows target list does not replace either native run. No test result was inferred from cross-target availability.

## Next evidence required

Run both commands above on a native Windows runner, record the Windows version, filesystem, Rust toolchain, target HEAD, feature set, permissions, full test name, and actual test count. Confirm the fixture and all three multi-reader scenarios execute with six forced fallbacks. Only a reproduced production cursor interference result can unlock T5; fixture or permission failures must be distinguished before considering a production change.

## T5 decision

T5 is not executed. The native prerequisite was unavailable, so neither independent-handle reopen nor an `Unsupported` behavior change is supported by evidence. Existing Windows fallback production code remains unchanged.
