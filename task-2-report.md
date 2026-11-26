# T2 Follow-up Report

## RLF-TEST-WINDOWS-LATE-BINDING

The Windows fault injection in `LocalTreeRetainedResource::binding_matches_identity` was configured for call 2. That call occurs before the child ACL mutation (the staged-root precheck). The post-child, pre-root-policy check is call 3: call 1 is the retained-entry preparation check, call 2 is the root precheck, and call 3 follows child mutation. The injection now targets call 3.

The existing Windows regression still asserts all required effects: a `ChangedDuringRead` error with `Mixed` state, changed child DACL, unchanged root DACL, a readable stage entry with the original identity, and successful cleanup/recovery. The Unix owner-policy lifecycle coverage remains untouched.

### Verification evidence

- `cargo test --locked --features test-support --test owner_policy_lifecycle_tests` — passed, 38 tests.
- `cargo test --locked --features test-support --test windows_staged_sync_tests test_owner_policy_late_stage_binding_mismatch_blocks_root_acl_mutation -- --exact --nocapture` on Linux — compiled the Linux test target but ran 0 tests because this integration test is gated on Windows.
- `cargo test --target x86_64-pc-windows-gnu --locked --features test-support --test windows_staged_sync_tests test_owner_policy_late_stage_binding_mismatch_blocks_root_acl_mutation --no-run` — blocked before compiling this crate's Windows test: `alloca` could not find `x86_64-w64-mingw32-gcc`; `windows-sys` also could not find `x86_64-w64-mingw32-dlltool`.

The corrected Windows regression could not be executed here, so a native RED/GREEN run and target Windows CI evidence remain outstanding. Before this fix, the injected mismatch fires on call 2 at the root precheck, before child mutation. The regression then reports `Preserved` instead of the asserted `Mixed` state, and the child DACL remains unchanged. The fix changes only the injection ordinal. Windows execution is required to confirm the expected RED and GREEN outcomes.

## Preserved T2 work

The named-entry depth fix from commit `51f38fb0244b5f88ec5ca08d259d9fc68f93058f` and the nameless-error regression remain unchanged.
