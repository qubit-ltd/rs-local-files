# RLF4 Windows staging and sealed cleanup evidence

## Status summary

| Capability / test scope | Historical evidence | Native environment and result | Applicability to implementation baseline `c5639417080648da263216f698893c3a6c3c7f2d` |
| --- | --- | --- | --- |
| Rooted staging cleanup: `test_rooted_staging_abort_preserves_competitor_at_old_name`, `test_rooted_staging_drop_preserves_competitor_at_old_name`, `test_rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name`; selected sealed cleanup tests | [`251e103bd9c60bf749e60453e688cf9ac4bde6f2`](https://github.com/qubit-ltd/rs-local-files/commit/251e103bd9c60bf749e60453e688cf9ac4bde6f2), [run 42508087870](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870) on 2026-11-08 | Windows Server 2025, NTFS; default and `--all-features`: staging 3/3 each, sealed cleanup 7/7 (default) and 11/11 (`--all-features`); both relevant Windows jobs passed | Historical result only. This SHA is an ancestor of the implementation baseline. Later source/test changes include staged-directory sync and filesystem I/O paths; no native execution at the implementation baseline is recorded here. |

The run's overall conclusion was failure because the coverage job measured 94.98% function coverage against a >=95.00% threshold. See the job-specific results and target-revision evidence boundary below.

## Historical review baseline status (2026-11-08)

The review baseline below records what was known before the later native run. It is not the status of the implementation revision documented afterward.

**待验证 / unverified at the historical review baseline `f55f4706d157585e1ef751e8dcb384259244b5cb` (2026-11-08).** That review ran on Linux, not Windows. No native Windows command was run in this worktree. Windows rename/delete sharing behavior, volume type, relevant account permissions, current-revision execution count, and current native logs are **未取得 / not obtained**. No implementation defect is inferred from the absence of this evidence.

| Field | Historical review baseline |
| --- | --- |
| Worktree HEAD | `f55f4706d157585e1ef751e8dcb384259244b5cb` |
| Host | Linux `6.8.0-147-generic`, x86_64 (`uname -a`) |
| Windows OS / native toolchain | 未取得；无 Windows 原生执行环境 |
| Windows volume type / relevant account permissions | 未取得 |
| Current-revision test count and result | 未取得；命令未运行 |
| Current-revision log | 未取得 |

## Historical Git record

Git history contains `5147bb05d26ea3ddf3364a1e2f9304f2b876dc29`, adding three Windows-gated staging cleanup tests and an evidence note. That note explicitly says the native Windows tests were not run and reports zero executions because the host was Linux. It names `rooted_staging_abort_preserves_competitor_at_old_name`, `rooted_staging_drop_preserves_competitor_at_old_name`, and `rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name`. This is evidence of a test addition and a pending state, not a Windows pass.

The review-baseline workflow configured those staging tests on Windows. At that revision, their function names use a `test_` prefix; the historical Git record uses the unprefixed names. The workflow helper tries both exact names and requires one test body to execute successfully, so it supports either tree. Configuration alone did not establish that a run occurred. The relevant sealed cleanup cases in `tests/local_temp_file_tests.rs` include `windows_sealed_cleanup_required_private_rooted`, `windows_sealed_cleanup_drop_removes_owned_entry`, `windows_sealed_cleanup_rejects_replacement`, and fallback cleanup/retry cases. Their native results are recorded below.

## Native verification requested at review time

On native Windows with a recorded local volume and fixture permissions, the workflow's staging commands are:

```text
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_abort_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_drop_preserves_competitor_at_old_name -- --exact --nocapture
cargo test --locked --test windows_rooted_staging_cleanup_tests test_rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name -- --exact --nocapture
```

Repeat each with `--all-features`. List and run selected `windows_sealed_cleanup_*` tests from `local_temp_file_tests` in both feature configurations as well. Retain full logs and actual execution counts. A fixture blocked by Windows sharing or permissions must be recorded as such and classified before drawing a product conclusion. This describes the evidence still needed at the historical review baseline; the later result below applies only to the named SHA and test groups.

## Earlier native CI result for the review baseline

The GitHub Actions run for the exact review baseline `b15ac95bd3a81b18c0457aef80f5745ef27bd142` is [run 42491360435](https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435), workflow `Rust CI`, event `push`, dated 2026-11-08. Its `rust-ci / verify (windows-latest)` job used hosted runner `GitHub Actions 1000094458` and ran the Windows verification orchestration. The job failed during its initial build suite before the extra script could invoke any target staging tests: Cargo reported `cannot update the lock file ... Cargo.lock because --locked was passed to prevent this`; the orchestrator reported `cargo build --locked --workspace --all-targets --all-features failed`. The log identifies Microsoft Windows Server 2025 build 10.0.26100, image `windows-2025-vs2026` version `20261103.298.1`, Rust target `x86_64-pc-windows-msvc`, and the hosted `runneradmin` account context. It does not establish the volume type or fixture permissions, and contains no execution results for the staging or sealed cleanup tests. Therefore this run is evidence that a Windows job for that exact SHA started, but it is not test evidence for RLF-003. No implementation defect is inferred from this infrastructure/build failure.

- Run: <https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435>
- Job: <https://github.com/qubit-ltd/rs-local-files/actions/runs/42491360435/job/128764239694>
- Environment: Windows Server 2025 (10.0.26100), `windows-2025-vs2026` image, Rust `x86_64-pc-windows-msvc`, hosted `runneradmin` context; volume type and fixture permissions not recorded.
- Result: no target tests executed; Windows behavior remains unverified.
- Remaining environment: a successful native Windows run with OS build, local volume type, relevant account permissions, feature sets, exact executed test names/counts, and complete logs.

## Lockfile diagnosis and correction

In the implementation recorded at [`251e103bd9c60bf749e60453e688cf9ac4bde6f2`](https://github.com/qubit-ltd/rs-local-files/commit/251e103bd9c60bf749e60453e688cf9ac4bde6f2) on 2026-11-08, the repository's path-dependency pin mapped `../rs-redact` to commit `04f4bef3ac9a391cc220438918dcb1451a1963a6`, whose package version is `0.10.0`. The committed root and fuzz lockfiles and three external-consumer lock fixtures instead recorded the path package as `0.10.1`. The repository CI helper's pinned checkout and `--locked`/`--frozen` builds exposed this mismatch: Cargo required a lock update before it could build. That revision aligned those five lock records with the declared pinned path package (`0.10.0`); it did not change a Cargo.toml dependency declaration or add/upgrade a dependency. The repository record for that revision says local `ci-check.sh` passed with the corrected lockfiles. Its Windows CI result is detailed below; neither claim is evidence of coverage at the later implementation baseline.

## Recorded Windows verification at revision 251e103 (2026-11-08)

The following is a historical CI result for the exact revision [`251e103bd9c60bf749e60453e688cf9ac4bde6f2`](https://github.com/qubit-ltd/rs-local-files/commit/251e103bd9c60bf749e60453e688cf9ac4bde6f2), not a current-revision result. Run [42508087870](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870) was triggered by push on 2026-11-08 at 18:04 UTC for that SHA. The `Conditional write contracts / conditional (windows-latest)` job [128814983611](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870/job/128814983611) and `rust-ci / verify (windows-latest)` job [128814983552](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870/job/128814983552) both succeeded. The overall workflow run concluded **failure** because `rust-ci / coverage` [128818044541](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870/job/128818044541) failed its function coverage threshold (94.98% against >=95.00%); these job-specific Windows passes must not be reported as an overall run pass.

- Native test job: [Windows conditional-write job](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870/job/128814983611).
- Full Windows verification job: [Windows verify job](https://github.com/qubit-ltd/rs-local-files/actions/runs/42508087870/job/128814983552).
- OS: Microsoft Windows Server 2025 Datacenter, version 10.0.26100 (build 26100).
- Volume: `D:`; NTFS, fixed disk; disk 1, SAS, Microsoft Virtual Disk, MBR.
- Account: `runnervmm7ud1\runneradmin`. Workspace and temporary-directory ACL output recorded full control for Administrators and SYSTEM, inherited CREATOR OWNER full control, and BUILTIN Users read/execute plus add-subdirectory/write-data access.
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, target `x86_64-pc-windows-msvc`.
- Staging cleanup: all three exact tests listed in “Native verification requested at review time” ran and passed in the default and `--all-features` configurations (3/3 in each configuration).
- Sealed cleanup: the selected `windows_sealed_cleanup_*` tests ran and passed in both configurations (7/7 default; 11/11 all-features).
- Result: the targeted staging and sealed cleanup tests passed on the recorded native Windows environment. The workflow job logs above contain the recorded environment and command/test output.

The linked Windows job logs were retrieved and independently inspected for this update with `gh run view`. They show the recorded environment and all staging and sealed-cleanup test commands and results below. This verifies the claims against the CI logs, while leaving the coverage failure and overall run conclusion explicit.

The Windows verify job also succeeded for its CI verification scope, but neither it nor the targeted native job establishes Windows coverage at a later revision. Target-revision coverage remains unverified until these exact or otherwise explicitly selected tests are run natively at that target SHA. This record does not establish behavior for the unlisted Windows cases, RLF3 enumeration, or other platforms.
