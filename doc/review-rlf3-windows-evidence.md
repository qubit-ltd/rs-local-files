# RLF3 Windows rooted enumeration evidence

## Current status (2026-11-08)

**待验证 / unverified at the implementation revision.** This review is running on Linux, not Windows. No native Windows command was run in this worktree, so current-revision behavior, volume type, and Windows account permissions are **未取得 / not obtained**. A Windows target or a Linux test filter would not establish native behavior.

| Field | Current review |
| --- | --- |
| Worktree HEAD | `f55f4706d157585e1ef751e8dcb384259244b5cb` |
| Host | Linux `6.8.0-147-generic`, x86_64 (`uname -a`) |
| Windows OS / native toolchain | 未取得；无 Windows 原生执行环境 |
| Windows volume type / relevant account permissions | 未取得 |
| Current-revision test count and result | 未取得；命令未运行 |
| Current-revision log | 未取得 |

The relevant current integration test is `rooted_root_enumerators_have_independent_cursors_across_native_batches` in `tests/review_windows_root_enumeration_tests.rs`. Workflow configuration is not execution evidence.

## Historical record (not current-revision evidence)

Git history contains commit `102b13090d45e391a70ee205e284f6388ae1d105`, which added a record claiming that at source revision `daa0e0282a575885c904182cc1c456ba2e28329f`, the exact then-current fail-closed enumeration test passed once with `--no-default-features` and once with `--all-features`. The recorded commands were:

```text
cargo test --locked --no-default-features --lib local::internal::windows_rooted::directory::fallback_tests::test_root_enumeration_fallback_fails_closed_without_independent_handle -- --exact --nocapture
cargo test --locked --all-features --lib local::internal::windows_rooted::directory::fallback_tests::test_root_enumeration_fallback_fails_closed_without_independent_handle -- --exact --nocapture
```

The record reports `1 passed` in each run and links [workflow run 41692830948](https://github.com/qubit-ltd/rs-local-files/actions/runs/41692830948/jobs/126115550000). It states the Windows runner OS and toolchain, but volume type and relevant account permissions were not recorded. The run concerns that recorded revision and does not establish behavior at this worktree HEAD.

The historical Git document is the available record. During this task, the linked workflow job URL was queried and returned HTTP 404, so its runner log could not be independently retrieved. A current-run log was not obtained. Historical claims are retained as claims from that record, not promoted to current verification.

## Native verification still needed

On a native Windows checkout of the target revision, first confirm the exact intended test names with `-- --list`, then execute the applicable precise enumeration test in both feature configurations. Record actual test count, OS, toolchain, volume type, relevant fixture permissions, revision, and complete runner log. Until then, Windows enumeration remains待验证.
