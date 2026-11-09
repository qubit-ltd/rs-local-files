# RLF5-Q01 Windows access evidence

## Result

The requested Windows scenario matrix is verified on the implementation
revision. The native run executed the new pre-existing-read-only regression,
the partial-failure cleanup regression, and the exact-DACL and transition
tests in both default and all-features configurations.

## Implementation-SHA native CI evidence

- Revision: `5291d57ab1889b80559283610d0bb5f9f671d279`.
- Workflow: [Rust CI run 42596834146](https://github.com/qubit-ltd/rs-local-files/actions/runs/42596834146), completed successfully on 2026-11-09.
- Native verification job: [rust-ci / verify (windows-latest), job 129091425125](https://github.com/qubit-ltd/rs-local-files/actions/runs/42596834146/job/129091425125), successful. Its log shows each exact test below passed once with default features and once with `--all-features`:
  - `tests::temp::internal::privacy_windows_tests::test_owner_policy_windows_create_apply_verify_uses_exact_dacl_mask`
  - `tests::temp::internal::privacy_windows_tests::test_owner_policy_windows_directory_create_apply_verify_uses_exact_dacl_mask`
  - `tests::lf3_privacy_windows_access_transition_tests::test_review_windows_access_transition_preserves_readonly_attribute_and_acl`
  - `tests::lf3_privacy_windows_access_transition_tests::test_review_windows_owner_policy_preserves_preexisting_readonly_attribute`
  - `tests::lf3_tree_publish_tests::test_windows_access_failure_after_child_mutation_reports_mixed_and_keeps_stage`
- Additional native job: [Conditional write contracts / conditional (windows-latest), job 129091425344](https://github.com/qubit-ltd/rs-local-files/actions/runs/42596834146/job/129091425344), successful. It ran the shared conditional suites and recorded the native environment.
- Environment recorded by the conditional Windows job: Microsoft Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`; workspace volume D:, NTFS, fixed virtual disk; identity `runnervmm7ud1\runneradmin`; Rust `1.94.0-x86_64-pc-windows-msvc`. The log records workspace and runner-temp ACLs. The successful native test output is stronger behavioral evidence than the ACL listing alone.

## Scenario coverage

| Required scenario | Test and behavior | Status |
| --- | --- | --- |
| Apply and observe exact Windows DACL masks | `test_owner_policy_windows_create_apply_verify_uses_exact_dacl_mask` and `test_owner_policy_windows_directory_create_apply_verify_uses_exact_dacl_mask` passed in default and all-features configurations. `test_owner_policy_windows_apply_without_write_dac_fails_without_acl_expansion` checks denied mutation without ACL expansion. | Verified on implementation SHA. |
| Toggle read-only policy and restore the DACL | `test_review_windows_access_transition_preserves_readonly_attribute_and_acl` checks read-only and restored DACLs and observes that writes remain denied while `FILE_ATTRIBUTE_READONLY` remains set. | Verified on implementation SHA. It does not establish write access after DACL restoration alone. |
| Begin with a pre-existing `FILE_ATTRIBUTE_READONLY`, then convert/restore owner policy | `test_review_windows_owner_policy_preserves_preexisting_readonly_attribute` checks the attribute and denied write after DACL restoration, clears the attribute as explicit fixture cleanup, then verifies a write handle opens. | Verified on implementation SHA. |
| Fail partway through child access application, preserve/recover the stage | `test_windows_access_failure_after_child_mutation_reports_mixed_and_keeps_stage` injects failure after a child mutation, asserts `Mixed`, retains and observes the stage, retries to `CurrentUserOnly`, then explicitly calls `stage.cleanup()`. It does not attempt publication, so it makes no publication-state claim. | Verified on implementation SHA. |
| Close handles and explicitly clean up temporary/staged resources | Native CI executes `windows_rooted_staging_cleanup_tests` and Windows sealed-cleanup selections in `local_temp_file_tests`, including drop cleanup, replacement rejection, retries, and read-only fallback cleanup. The partial owner-policy failure test also explicitly cleans up after recovery. | Verified on implementation SHA. |

The exact access-transition and partial-failure tests above passed once under
default features and once under `--all-features`; the two exact-DACL tests did
the same. The workflow and job logs are linked above. The prior baseline run
remains available for historical context at [Rust CI run 42559482374](https://github.com/qubit-ltd/rs-local-files/actions/runs/42559482374); it is not used to claim verification of tests added in this implementation.

## Conclusion

Q01 is confirmed. Native Windows CI passed the exact regression tests under
both feature configurations at the implementation SHA. No implementation
defect was demonstrated by this review.
