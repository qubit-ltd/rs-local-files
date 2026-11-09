# RLF5-Q01 Windows access evidence

## Result

The baseline has successful native Windows evidence, but the full requested
scenario matrix is not yet verified. The evidence below is bound to
`a3aee6d04cfa3117f5217fa2df0adb4fe76bade4`; it does not validate the new
pre-existing-read-only regression added in this worktree. Q01 remains pending
until that exact test runs on Windows.

## Current-SHA native CI evidence

- Revision: `a3aee6d04cfa3117f5217fa2df0adb4fe76bade4`.
- Workflow: [Rust CI run 42559482374](https://github.com/qubit-ltd/rs-local-files/actions/runs/42559482374), completed successfully on 2026-11-09.
- Native verification job: [rust-ci / verify (windows-latest), job 128972520172](https://github.com/qubit-ltd/rs-local-files/actions/runs/42559482374/job/128972520172), successful. Its log shows nonzero execution of the Windows unit and integration tests under both `--no-default-features` and `--all-features`; the unit test harness reports 499 passed, zero failed for the no-default-features run.
- Additional native job: [Conditional write contracts / conditional (windows-latest), job 128972520176](https://github.com/qubit-ltd/rs-local-files/actions/runs/42559482374/job/128972520176), successful. It ran nonzero `windows_rooted_staging_cleanup_tests` and `local_temp_file_tests` selections under default and all-features configurations. The cleanup suite reported 7 passed with defaults and 11 passed with all features; each result was nonzero.
- Environment recorded by the conditional Windows job: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`; workspace volume D:, NTFS, fixed disk, NVMe; identity `runnervmm7ud1\runneradmin`; Rust `1.94.0-x86_64-pc-windows-msvc`. The log records workspace and runner-temp ACLs. Both contain inherited `BUILTIN\Administrators:(F)`, `NT AUTHORITY\SYSTEM:(I)(OI)(CI)(F)`, `CREATOR OWNER:(I)(OI)(CI)(IO)(F)`, and `BUILTIN\Users:(I)(CI)(AD)` plus `(I)(CI)(WD)` entries. The successful native test output is stronger behavioral evidence than the ACL listing alone.

## Scenario coverage

| Required scenario | Existing test and observed evidence at the baseline SHA | Status |
| --- | --- | --- |
| Apply and observe exact Windows DACL masks | `tests::temp::internal::privacy_windows_tests::test_owner_policy_windows_create_apply_verify_uses_exact_dacl_mask` and `...::test_owner_policy_windows_directory_create_apply_verify_uses_exact_dacl_mask`; both appear as `ok` in the native `verify` log. Also `...::test_owner_policy_windows_apply_without_write_dac_fails_without_acl_expansion` checks denied mutation without ACL expansion. | Verified on baseline SHA. |
| Toggle read-only policy and restore the DACL | `tests::lf3_privacy_windows_access_transition_tests::test_review_windows_access_transition_preserves_readonly_attribute_and_acl` checks read-only and restored DACLs and observes that writes remain denied while `FILE_ATTRIBUTE_READONLY` remains set. | Verified for the documented attribute-preserving result. It does not establish write access after DACL restoration alone. |
| Begin with a pre-existing `FILE_ATTRIBUTE_READONLY`, then convert/restore owner policy | No baseline test starts with that attribute set before policy conversion. A focused test, `tests::lf3_privacy_windows_access_transition_tests::test_review_windows_owner_policy_preserves_preexisting_readonly_attribute`, was added in this worktree. It checks the attribute and denied write after DACL restoration, clears the attribute as explicit fixture cleanup, then verifies a write handle opens. | New regression is defined but has no native Windows execution yet. |
| Fail partway through child access application, preserve/recover the stage | `tests::lf3_tree_publish_tests::test_windows_access_failure_after_child_mutation_reports_mixed_and_keeps_stage` injects failure after a child mutation, asserts `Mixed`, retains and observes the stage, then retries to `CurrentUserOnly`. This proves the access state and retry recovery; it does not attempt publication, so it makes no publication-state claim. | Verified on baseline SHA for access/recovery. The test now explicitly calls `stage.cleanup()` after recovery; this added cleanup assertion awaits native execution. |
| Close handles and explicitly clean up temporary/staged resources | Baseline CI executes `windows_rooted_staging_cleanup_tests` and Windows sealed cleanup selections in `local_temp_file_tests`, including drop cleanup, replacement rejection, retries, and read-only fallback cleanup. | General Windows cleanup is verified. Cleanup after the partial owner-policy failure is newly asserted and awaits native execution. |

## Conclusion and remaining evidence

No implementation defect is demonstrated by this review. The existing native
run confirms the documented behavior that restoring the DACL leaves a
read-only file attribute in place. It does not cover a file that already had
that attribute before owner-policy conversion, and it predates the explicit
cleanup assertion added to the partial-failure recovery test. The new exact
test name for CI wiring is
`tests::lf3_privacy_windows_access_transition_tests::test_review_windows_owner_policy_preserves_preexisting_readonly_attribute`.
The T4 integration step must run it on native Windows and record a nonzero
result before Q01 can be reported fully verified. Linux execution or Windows
cross-compilation alone would not supply that evidence.
