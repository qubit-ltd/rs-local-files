# FreeBSD Compatibility Status

## Current status

FreeBSD is temporarily unsupported by `qubit-local-files`. The current CI evidence is from FreeBSD 14.3 in GitHub Actions run [41173303438](https://github.com/qubit-ltd/rs-local-files/actions/runs/41173303438), revision `f72a5b26b0b598adc2d4f0d7edcbc0d019537892` (2026-10-31).

“Unsupported” means the project does not currently promise that the APIs below work on FreeBSD. It does not mean every API is disabled or always returns `Unsupported`: reads, metadata queries, cleanup, and some feature-specific tests did succeed. Do not rely on a successful call or a build check as evidence that the complete operation contract is supported.

The CI runner did not record the backing filesystem type or mount options. Therefore errors such as `Operation not supported` are attributed to the native operation as observed in that runner, not to a specific FreeBSD filesystem.

## API areas with failing native tests

| API area | Observed failures | What the run showed |
| --- | --- | --- |
| `LocalFileSystem::copy` and recursive copy | `test_host_facade_workflow_covers_copy_and_temporary_resource_paths`, `test_local_file_system_copy_unifies_file_and_directory_copy`, `test_directory_copy_statistics_match_publication_actions`, `test_scoped_copy_follows_in_scope_directory_link` | Staged copy failed while installing the destination with `Unsupported`: the Host no-replace move path is not implemented for FreeBSD. Other failures in this group stopped while applying destination metadata. |
| `LocalFileSystem::rename` and rooted rename | `test_root_authority_manages_descendant_entries`, `test_rooted_local_file_system_rename_respects_overwrite`, `test_local_file_system_rename_respects_overwrite_policy` | Rooted no-replace rename is explicitly unsupported by the current FreeBSD implementation. Host no-replace file moves also use an unsupported branch. A test expecting the unsupported capability to be reported as `RequirementNotMet` instead received `AlreadyExists`; this error-classification case needs separate review. |
| Atomic writer commit and replacement | `test_atomic_commit_state_publishes_host_staging`, `test_atomic_commit_state_publishes_rooted_staging`, `test_following_write_modes_keep_existing_contract`, `test_rooted_local_file_system_writer_follows_final_symlink` | Commit reached `ApplyDestinationMetadata` and the native filesystem returned errno 45, `Operation not supported`; the outcome remained `NotPublished`. The runner did not capture enough mount information to determine which metadata operation or filesystem capability caused it. |
| Temporary file/directory `persist` and `keep` | `test_local_temp_file_persist_reports_atomic_rename`, `test_rooted_temp_file_persist_publishes_absent_target`, `test_rooted_temp_directory_persist_supports_new_and_overwrite_targets`, `test_creation_parent_is_a_snapshot_after_close_and_keep` | No-replace installation returned `Unsupported`; these tests cannot establish the publication/retention contract on FreeBSD. |
| Staged directory `publish_new` | `successful_publish_keeps_stage_contents_at_target`, `lf_review_tree_publish_depth_limit_retains_stage`, `test_published_outcome_transfers_secondary_cleanup_recovery_without_retracting_publication` | Publication requires a supported no-replace namespace operation. The current rooted directory rename path reports `Unsupported` or `RequirementNotMet` before publication. |
| Copy/write metadata preservation | `test_successful_host_copy_reports_applied_permissions`, `test_metadata_policy_selects_permissions`, `rooted_copy_metadata_failure_matrix_preserves_sources_and_proven_destination_state` | Destination metadata application returned errno 45 (`Operation not supported`). This is a runner observation; the exact filesystem and unsupported metadata primitive were not recorded. |
| Deleted current working directory diagnostics | `test_deleted_process_cwd_preserves_constructor_and_pair_operation_context` | The test expected `NotFound` but observed `Unsupported` while resolving the deleted process working directory. This is a platform error-mapping difference, not evidence that all path resolution is unavailable. |
| Temporary cleanup after descriptor exhaustion | `test_temp_creation_descriptor_budget_child` | Recovery cleanup returned `DirectoryNotEmpty`; the sandbox contained an entry whose identity had not been established, so safe cleanup could not remove it. The run does not yet establish whether this is a FreeBSD implementation defect or a runner-specific behavior. |

These are representative failing tests from the full run, not a complete compatibility guarantee or an exhaustive list of failing test cases. The run also had failures in broader workflow, copy, rename, temporary-resource, and tree-publication suites because those suites exercise the same unsupported operations.

## The eight failing all-features doctests

The all-features doctest phase reported 145 passed, 8 failed, and 5 ignored. The failures were:

1. `src/documentation_tests.rs` line 936 — English user-guide staged backup example; required durable publication returned `RequirementNotMet`.
2. `src/documentation_tests.rs` line 2557 — Chinese user-guide counterpart; same required rename guarantee was unavailable.
3. `LocalTreePublishOptions` — ordinary staged-directory publication was unsupported.
4. `LocalCopyOutcome` — Host copy could not perform native no-replace destination installation.
5. `LocalCopyStats` — same Host copy installation failure.
6. `LocalPersistCleanupState` — temporary-file persistence could not install without replacement.
7. `LocalRenameOutcome` — no-replace rename was unsupported.
8. `LocalTreePublishOutcome` — staged-directory publication could not meet the requested rename guarantee.

The FreeBSD-specific limitations in these examples are handled explicitly in the examples; supported-platform branches continue to exercise the success path.

## Operations that did pass

The same FreeBSD run successfully exercised the FreeBSD xattr short-read/atomic-preservation test and the conditional-write `Unsupported` contract for the unsupported `Content` protocol. The xattr-focused `test_attribute_read` checks also passed. Those results cover only the named contracts; they do not imply support for the failing operation families above.

## Why we do not emulate no-replace rename

The current Rooted implementation has no FreeBSD atomic descriptor-relative no-replace rename primitive. A check-then-rename sequence would introduce a race: another process could create the destination after the check, and ordinary rename may replace it. The project therefore keeps the operation rejected instead of weakening the no-replace guarantee. FreeBSD's [`renameat(2)` manual](https://man.freebsd.org/cgi/man.cgi?manpath=FreeBSD+14.1-RELEASE&query=renameat) documents ordinary rename behavior when the destination already exists.

Some file-install and xattr helpers already have FreeBSD-specific code, but that does not cover every Host and Rooted API path. Compatibility must be established per operation family and per native filesystem capability.

## Re-enabling support

FreeBSD support can be reconsidered after the project has verified safe no-replace primitives for the affected Host and Rooted operations, captured the CI filesystem and mount details for metadata failures, resolved the descriptor-exhaustion cleanup case, and passed the relevant native tests and doctests on a declared FreeBSD filesystem. Until then, FreeBSD remains outside the supported platform set.
