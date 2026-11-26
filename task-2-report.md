# T2 Follow-up Report

## RLF-TEST-WINDOWS-LATE-BINDING

The ordinal-based fault was removed. The checked-in mutation flow has four calls to `binding_matches_identity`:

1. `access.rs:759`: retained-entry preparation, before mutation.
2. `access.rs:800`: Windows root-handle precheck, before child mutations.
3. `access.rs:952`: after the retained-child mutation loop and before applying the root ACL.
4. `access.rs:1016`: after applying the root ACL.

The earlier report selected occurrence 3, which maps to call site 3 in this checked-out flow; occurrence 4 is after root ACL application. Since an ordinal depends on the entire invocation path and reviewers traced it differently, the regression now uses a dedicated selector consumed immediately at the post-child/pre-root identity-check result. Its location is phase-specific and has no occurrence counter. Moving the injection before child mutation causes the `Mixed` state and changed-child-DACL assertions to fail; moving it after root mutation causes the unchanged-root-DACL assertion to fail.

The regression retains assertions for `ChangedDuringRead`, `Mixed`, changed child DACL, unchanged root DACL, original stage identity and payload, and successful cleanup. The Unix owner-policy lifecycle tests are unchanged.

## Evidence and checkpoints

- **FIRST_CHECK:** Traced all four call sites above and the sole old `take_test_support_on_nth` selector. The post-child/pre-root boundary is call site 3, but the injection is now attached directly to that phase rather than an ordinal.
- **RED:** Changed the Windows test to the new phase-specific selector before adding its consumer. This is expected to fail on Windows because the current implementation does not consume that selector and proceeds to mutate the root. The test could not be executed in this environment, so this is a reasoned RED expectation, not a runtime RED result.
- **GREEN:** `cargo test --locked --features test-support --test owner_policy_lifecycle_tests` — passed, 38 tests. `git diff --check` — passed.
- **Windows execution limitation:** The Linux invocation of the Windows integration test is cfg-gated and runs zero tests. Cross-target `cargo test --target x86_64-pc-windows-gnu ... --no-run` stops in dependencies before compiling this crate: `alloca` cannot find `x86_64-w64-mingw32-gcc`, and `windows-sys` cannot find `x86_64-w64-mingw32-dlltool`. Native Windows RED/GREEN evidence remains outstanding.
- **COMMITTED:** Follow-up commit recorded separately in the Paseo checkpoint message; earlier commits are preserved.

## Preserved T2 work

The named-entry depth fix from commit `51f38fb0244b5f88ec5ca08d259d9fc68f93058f` and the nameless-error regression remain intact.
