# RLF4 Windows staging and sealed cleanup evidence

## Current status (2026-11-08)

**待验证 / unverified at the implementation revision.** This review is running on Linux, not Windows. No native Windows command was run in this worktree. Windows rename/delete sharing behavior, volume type, relevant account permissions, current-revision execution count, and current native logs are **未取得 / not obtained**. No implementation defect is inferred from the absence of this evidence.

| Field | Current review |
| --- | --- |
| Worktree HEAD | `f55f4706d157585e1ef751e8dcb384259244b5cb` |
| Host | Linux `6.8.0-147-generic`, x86_64 (`uname -a`) |
| Windows OS / native toolchain | 未取得；无 Windows 原生执行环境 |
| Windows volume type / relevant account permissions | 未取得 |
| Current-revision test count and result | 未取得；命令未运行 |
| Current-revision log | 未取得 |

## Historical Git record

Git history contains `5147bb05d26ea3ddf3364a1e2f9304f2b876dc29`, adding three Windows-gated staging cleanup tests and an evidence note. That note explicitly says the native Windows tests were not run and reports zero executions because the host was Linux. It names `rooted_staging_abort_preserves_competitor_at_old_name`, `rooted_staging_drop_preserves_competitor_at_old_name`, and `rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name`. This is evidence of a test addition and a pending state, not a Windows pass.

The current workflow configures those staging tests on Windows. The committed test names are the historical unprefixed names below. A pre-existing unstaged worktree change renames them with a `test_` prefix; the workflow helper now tries both exact names and requires one test body to execute successfully, so it supports either tree. Configuration does not establish that a run occurred. The current relevant sealed cleanup cases in `tests/local_temp_file_tests.rs` include `windows_sealed_cleanup_required_private_rooted`, `windows_sealed_cleanup_drop_removes_owned_entry`, `windows_sealed_cleanup_rejects_replacement`, and fallback cleanup/retry cases. Their current native execution results are未取得.

## Native verification still needed

On native Windows with a recorded local volume and fixture permissions, the workflow's staging commands are:

```text
cargo test --locked --test windows_rooted_staging_cleanup_tests rooted_staging_abort_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests rooted_staging_drop_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name -- --exact --nocapture
```

Repeat each with `--all-features`. List and run selected `windows_sealed_cleanup_*` tests from `local_temp_file_tests` in both feature configurations as well. Retain full logs and actual execution counts. A fixture blocked by Windows sharing or permissions must be recorded as such and classified before drawing a product conclusion. Until then, staging cleanup and sealed cleanup remain待验证.
