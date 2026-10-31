# RLF3 copy deadline verification

## RLF3-001 red/green regression

The pre-fix regression was run against baseline `50e168b4853b777752a75f31a77900bbb6cd476d` in a temporary worktree with only the new regression test and its module declaration applied. The helper forwarding test was omitted in that baseline copy because the helper does not exist before the fix.

```text
cargo test --locked --lib directory_copy_deadline_tests::directory_copy_deadline_covers_handle_path_prevalidation -- --nocapture
```

Expected red result: the single test failed at `the deadline should expire at the first resolver checkpoint`; result was `0 passed; 1 failed; 474 filtered out` (exit code 101). This demonstrates that the old handle prevalidation performed work instead of honoring the already expired copy deadline.

After the fix, the integrated worktree ran:

```text
cargo test --locked --lib directory_copy_deadline_tests -- --nocapture
```

`2 passed; 0 failed; 474 filtered out`. Both the resolver checkpoint test and the expired-start forwarding test executed.

```text
cargo test --locked --test directory_tests copy_deadline -- --nocapture
```

`1 passed; 0 failed; 12 filtered out`. The test verifies an unchanged overwrite target, no parent creation, `Copy` operation, `Io`/`TimedOut`, and zero copied-file statistics for zero deadline.

Adjacent contracts also passed:

```text
cargo test --locked --test component_policy_boundary_tests
```

`2 passed; 0 failed`.

```text
cargo test --locked --all-features --test review_copy_delete_deadline_tests
```

`3 passed; 0 failed`.

## Limitations

The pre-fix test was staged in a disposable baseline worktree because Cargo path dependencies must resolve to the clean sibling repositories. No pre-existing sibling worktree with uncommitted changes was used or modified. Full current-crate validation is recorded in the implementation report; no downstream all-features suite is part of this crate's CI.
