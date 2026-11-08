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

The current workflow configures those staging tests on Windows. At this revision, their function names use a `test_` prefix; the historical Git record uses the unprefixed names. The workflow helper tries both exact names and requires one test body to execute successfully, so it supports either tree. Configuration does not establish that a run occurred. The current relevant sealed cleanup cases in `tests/local_temp_file_tests.rs` include `windows_sealed_cleanup_required_private_rooted`, `windows_sealed_cleanup_drop_removes_owned_entry`, `windows_sealed_cleanup_rejects_replacement`, and fallback cleanup/retry cases. Their current native execution results are未取得.

## Native verification still needed

On native Windows with a recorded local volume and fixture permissions, the workflow's staging commands are:

```text
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_abort_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_drop_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name -- --exact --nocapture
```

Repeat each with `--all-features`. List and run selected `windows_sealed_cleanup_*` tests from `local_temp_file_tests` in both feature configurations as well. Retain full logs and actual execution counts. A fixture blocked by Windows sharing or permissions must be recorded as such and classified before drawing a product conclusion. Until then, staging cleanup and sealed cleanup remain待验证.

## Existing native CI result for the review baseline

The GitHub Actions run for the exact review baseline `b15ac95bd3a81b18c0457aef80f5745ef27bd142` is [run 42491360435](https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435), workflow `Rust CI`, event `push`, dated 2026-11-08. Its `rust-ci / verify (windows-latest)` job used hosted runner `GitHub Actions 1000094458` and ran the Windows verification orchestration. The job failed during its initial build suite before the extra script could invoke any target staging tests: Cargo reported `cannot update the lock file ... Cargo.lock because --locked was passed to prevent this`; the orchestrator reported `cargo build --locked --workspace --all-targets --all-features failed`. The log identifies Microsoft Windows Server 2025 build 10.0.26100, image `windows-2025-vs2026` version `20261103.298.1`, Rust target `x86_64-pc-windows-msvc`, and the hosted `runneradmin` account context. It does not establish the volume type or fixture permissions, and contains no execution results for the staging or sealed cleanup tests. Therefore this run is evidence that a current-SHA Windows job started, but it is not test evidence for RLF-003. No implementation defect is inferred from this infrastructure/build failure.

- Run: <https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435>
- Job: <https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435/job/128764239694>
- Environment: Windows Server 2025 (10.0.26100), `windows-2025-vs2026` image, Rust `x86_64-pc-windows-msvc`, hosted `runneradmin` context; volume type and fixture permissions not recorded.
- Result: no target tests executed; Windows behavior remains unverified.
- Remaining environment: a successful native Windows run with OS build, local volume type, relevant account permissions, feature sets, exact executed test names/counts, and complete logs.

## Lockfile diagnosis and correction

The repository's path-dependency pin maps `../rs-redact` to commit `04f4bef3ac9a391cc220438918dcb1451a1963a6`, whose package version is `0.10.0`. The committed root and fuzz lockfiles and three external-consumer lock fixtures instead recorded the path package as `0.10.1`. The repository CI helper's pinned checkout and `--locked`/`--frozen` builds exposed this mismatch: Cargo required a lock update before it could build. This implementation aligns those five lock records with the declared pinned path package (`0.10.0`); it does not change a Cargo.toml dependency declaration or add/upgrade a dependency. Full local `ci-check.sh` passes with the corrected lockfiles. A new native Windows Actions run is still needed to confirm the hosted job now reaches the Windows tests.
