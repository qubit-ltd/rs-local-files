# RLF-002 vectored I/O measurement

## Scope and decision

Added the `vectored_io` Criterion group to `benches/local_files.rs`. It measures the `LocalFileReader::read_vectored` / `LocalFileWriter::write_vectored` adapters beside direct `qubit_io::Input` / `Output` calls. The matrix covers 0, 1, 2, 8, 16, and 64 buffers, each with 64-byte and 4096-byte segments. There are 48 cases per run.

No production fast path was retained. A one-buffer conversion consistently sampled one additional allocation in the Local API path, but latency did not meet the acceptance rule reliably. A direct one-buffer write was repeatedly slower for 64-byte data, while the apparent 4096-byte writer gain ranged from 42–53% in the first two observations to 3% in the second repeat. Reader gains varied between 2–7%, with one repeat reversing direction. The result does not establish a repeatable 5% benefit without a greater-than-5% regression elsewhere in the small-buffer matrix.

## Environment

- Base `HEAD` before the T2 commit: `b9e4f54a1eca6c3ab9ea34a96254b2684842d5e6`.
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`, LLVM 21.1.8; Cargo 1.94.0.
- Criterion: 0.8.2, resolved under `Cargo.lock`.
- Host: Ubuntu 24.04.5 LTS, Linux `6.8.0-147-generic`, x86_64.
- CPU: dual-socket Intel Xeon E5-2680 v4 @ 2.40 GHz; 28 cores / 56 logical CPUs; frequency scaling was enabled (reported 88%).
- Benchmark fixtures use `tempfile` under `/tmp` (ext4 on `/dev/sda2`). The isolated Cargo target directory was on `/data` (ext4 on `/dev/md0`).
- The reader workload repeatedly rewinds and reads a cache-resident regular file. The writer workload appends to a regular file and truncates it between calls outside the measured interval. It does not sync data to durable media. These results describe warm local-file calls, not cold-storage or durability throughput.

## Method

The command builds/runs the existing `local_files` bench with locked dependencies and the `vectored_io` filter. Criterion used its default 3-second warmup and 100 samples per case (about 5 seconds of measured operation time per case). Each call was timed individually. File creation/open, payload/output setup, reader seek, writer truncate, preflight readback, and fixture cleanup were outside the timed interval. The timed closure black-boxes the returned progress count; the fixture preflight compares reader progress and ordered output, and checks writer output against the submitted payload.

A benchmark-only global allocator counter counts successful `alloc` and `realloc` calls. Its atomic sampling window encloses one Local API or direct trait call only; fixture, payload, and descriptor preparation occur before the window. It counts allocation calls, not bytes, and deallocations are not counted. For multiple buffers the direct `qubit_io` path itself prepares native slice descriptors, so its one allocation is expected; the Local API path adds its conversion allocation.

Commands and outcomes:

1. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files --no-run` — exit 0; produced `/data/working/qubit/rust-common/.cache-target-t2/release/deps/local_files-3f6864adb60d2ce2`.
2. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files -- vectored_io --save-baseline rlf002-before` — baseline run; all 48 cases completed. Criterion estimates are in the isolated target directory under `criterion/vectored_io/*/rlf002-before/`.
3. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files -- vectored_io --baseline rlf002-before` — repeat 1; all 48 cases completed.
4. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 /data/working/qubit/rust-common/.cache-target-t2/release/deps/local_files-3f6864adb60d2ce2 vectored_io --baseline rlf002-before --bench` — repeat 2 using the executable built in step 1; all 48 cases completed. This direct invocation avoided repeated Cargo package-cache lock contention.

The full stdout/stderr logs were captured at `/tmp/rs-local-files-rlf002-before.log`, `/tmp/rs-local-files-rlf002-round1.log`, and `/tmp/rs-local-files-rlf002-round2.log`. Two startup attempts were excluded: one direct invocation lacked `CARGO_TARGET_DIR` and therefore could not locate the saved Criterion baseline; another Cargo invocation was interrupted after one zero-buffer case when a competing workspace test acquired the shared Cargo lock. Neither supplied measurement data.

## Scoped allocation samples

The same allocation pattern appeared in all three complete runs (24 samples per run: two segment sizes × six buffer counts × two methods):

| Buffers | Local API read/write allocation calls | Direct trait read/write allocation calls |
| ---: | ---: | ---: |
| 0 | 0 / 0 | 0 / 0 |
| 1 | 1 / 1 | 0 / 0 |
| 2, 8, 16, 64 | 2 / 2 | 1 / 1 |

The one-buffer sample isolates an avoidable Local API descriptor allocation. For 2+ buffers, the direct trait path has its own descriptor allocation, so the Local API conversion adds one more allocation to that path.

## One-buffer latency results

Criterion's slope estimate and 95% confidence interval are shown in microseconds. The last column is `(Local API - direct trait) / Local API`; positive values mean the direct trait call was faster. These are observations, not optimized-code results.

| Operation / segment | Baseline: Local / direct | Repeat 1: Local / direct | Repeat 2: Local / direct | Direct-path change by run |
| --- | --- | --- | --- | --- |
| Read / 64 B | 0.938 [0.936, 0.941] / 0.899 [0.890, 0.910] | 0.934 [0.933, 0.935] / 0.983 [0.944, 1.026] | 0.934 [0.930, 0.940] / 0.914 [0.905, 0.925] | +4.1%, −5.3%, +2.1% |
| Read / 4096 B | 1.029 [1.028, 1.031] / 0.982 [0.980, 0.984] | 1.022 [1.020, 1.024] / 0.987 [0.974, 1.003] | 1.008 [0.992, 1.032] / 0.939 [0.935, 0.945] | +4.6%, +3.4%, +6.9% |
| Write / 64 B | 2.978 [2.968, 2.990] / 3.100 [3.019, 3.209] | 2.984 [2.978, 2.990] / 3.875 [3.608, 4.189] | 2.910 [2.903, 2.918] / 3.800 [3.482, 4.191] | −4.1%, −29.9%, −30.6% |
| Write / 4096 B | 5.043 [4.822, 5.256] / 2.912 [2.897, 2.929] | 5.925 [5.117, 6.769] / 2.808 [2.773, 2.853] | 3.464 [3.172, 3.767] / 3.357 [3.122, 3.626] | +42.3%, +52.6%, +3.1% |

The 64-byte writer result is a repeated regression for the direct path and would violate the no-repeatable-greater-than-5% regression guard for a general one-buffer fast path. The 4096-byte writer advantage did not recur in repeat 2. Reader results did not consistently reach the 5% threshold. No candidate implementation or production regression test was added.

## Measurement limits

The run host had other Cargo tests/checks and a separate benchmark active during parts of this work. This is visible in Criterion's outliers: across complete runs, recorded outliers reached 20–23 per 100 samples in individual cases; during repeat 2, many cases had 10–22% outliers. Some unchanged cases moved by tens of percent between runs. The direct 64-byte writer path also regressed in both repeat runs, while the 4096-byte writer delta collapsed in repeat 2. Those observations are why no latency improvement is claimed and no production code changed.

The benchmark executable's preflight passed in every completed run. The benchmark binary build passed; no production tests were needed because this task retained no production optimization. The broader integration gate remains T4's responsibility.
