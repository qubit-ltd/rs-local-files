# RLF-002 向量 I/O 测量证据

## 范围与决策

在 `benches/local_files.rs` 增加 `vectored_io` Criterion 组，比较 `LocalFileReader::read_vectored`／`LocalFileWriter::write_vectored` 适配器与直接调用 `qubit_io::Input`／`Output` trait。矩阵覆盖 0、1、2、8、16、64 个 buffer，每段 64 或 4096 字节，每轮共 48 个 case。

生产实现不保留单-buffer 快路径。单-buffer 时 Local API 每次会多分配一个描述符 Vec，但延迟结果没有稳定满足验收门槛；64 字节 writer 的 direct 路径在两轮重复中均出现反向回归，4096 字节 writer 的初始 42%–53% 差距在第二次重复缩至约 3%。reader 收益在不同轮间变化，未稳定达到 5%。因此没有可复现证据表明存在满足“至少 5% 收益且无可重复超过 5% 退化”的快路径。

## 环境

- T2 提交前基线：`b9e4f54a1eca6c3ab9ea34a96254b2684842d5e6`。
- Rust：`rustc 1.94.0 (4a4ef493e 2026-03-02)`，LLVM 21.1.8；Cargo 1.94.0。
- Criterion：锁定的 0.8.2。
- 主机：Ubuntu 24.04.5 LTS，Linux `6.8.0-147-generic`，x86_64。
- CPU：双路 Intel Xeon E5-2680 v4 @ 2.40 GHz，28 核／56 逻辑 CPU；频率缩放已开启（报告值 88%）。
- fixture 在 `/tmp` 下创建（ext4，`/dev/sda2`）；独立 Cargo target 在 `/data`（ext4，`/dev/md0`）。
- reader 重复 seek 并读取驻留缓存的普通文件；writer 对普通文件 append，并在计时区间外截断。测试不执行 sync。结果描述热缓存本地文件调用，不代表冷存储或持久化吞吐。

## 方法

每个 Criterion case 使用默认 3 秒 warmup 和 100 个样本（约 5 秒计时）。每次 API 调用单独计时。创建／打开 fixture、payload/output 和描述符准备、reader seek、writer truncate、读取回验和清理均位于计时区间外。返回进度数使用 `black_box`；计时前会比较 reader 进度及有序输出，并确认 writer 输出与 payload 一致。

bench 专用 global allocator 统计成功的 `alloc` 与 `realloc` 调用。原子采样窗口只包围一次 Local API 或直接 trait 调用；fixture、payload 和描述符准备在窗口外。计数是分配调用数而非字节数，不统计 dealloc。多 buffer 时，直接 `qubit-io` 路径本身会准备 native slice 描述符，因此直接路径的一次分配符合实现预期；Local API 适配器再增加一次转换分配。

执行命令与结果：

1. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files --no-run`：退出码 0，生成 `/data/working/qubit/rust-common/.cache-target-t2/release/deps/local_files-3f6864adb60d2ce2`。
2. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files -- vectored_io --save-baseline rlf002-before`：基线 48/48 case 完成；Criterion 估计保存在隔离 target 的 `criterion/vectored_io/*/rlf002-before/`。
3. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 cargo bench --locked --bench local_files -- vectored_io --baseline rlf002-before`：重复一，48/48 完成。
4. `CARGO_TARGET_DIR=/data/working/qubit/rust-common/.cache-target-t2 /data/working/qubit/rust-common/.cache-target-t2/release/deps/local_files-3f6864adb60d2ce2 vectored_io --baseline rlf002-before --bench`：重复二，运行前一命令构建的相同 benchmark binary，48/48 完成。设置 `CARGO_TARGET_DIR` 以复用已保存 baseline，避免 Cargo package-cache 锁反复中断测量。

完整 stdout/stderr 日志：`/tmp/rs-local-files-rlf002-before.log`、`/tmp/rs-local-files-rlf002-round1.log`、`/tmp/rs-local-files-rlf002-round2.log`。两次未纳入证据的启动：一次直接调用漏设 `CARGO_TARGET_DIR`，无法找到 Criterion baseline；另一次 Cargo 调用在竞争 workspace test 获得共享锁后中止，只完成一个 zero-buffer case。两者都没有提供有效测量样本。

## 分配采样

三轮中的分配模式一致；每轮 24 个采样（两种段长 × 六种 buffer 数 × 两种调用路径）：

| Buffer 数 | Local API 读／写分配调用 | 直接 trait 读／写分配调用 |
| ---: | ---: | ---: |
| 0 | 0 / 0 | 0 / 0 |
| 1 | 1 / 1 | 0 / 0 |
| 2、8、16、64 | 2 / 2 | 1 / 1 |

单-buffer 样本隔离出 Local API 可避免的一次描述符分配。2 个及以上 buffer 时，直接 trait 路径也会分配描述符，Local API 的转换再多一次分配。

## 单-buffer 延迟结果

列出 Criterion slope 估计及 95% 置信区间，单位微秒。差值为 `(Local API − direct trait) / Local API`；正数表示 direct 更快。数据是现有实现的对照，并非优化后的结果。

| 操作／段长 | 基线：Local / direct | 重复 1：Local / direct | 重复 2：Local / direct | Direct 相对每轮 Local 的变化 |
| --- | --- | --- | --- | --- |
| Read / 64 B | 0.938 [0.936, 0.941] / 0.899 [0.890, 0.910] | 0.934 [0.933, 0.935] / 0.983 [0.944, 1.026] | 0.934 [0.930, 0.940] / 0.914 [0.905, 0.925] | +4.1%、−5.3%、+2.1% |
| Read / 4096 B | 1.029 [1.028, 1.031] / 0.982 [0.980, 0.984] | 1.022 [1.020, 1.024] / 0.987 [0.974, 1.003] | 1.008 [0.992, 1.032] / 0.939 [0.935, 0.945] | +4.6%、+3.4%、+6.9% |
| Write / 64 B | 2.978 [2.968, 2.990] / 3.100 [3.019, 3.209] | 2.984 [2.978, 2.990] / 3.875 [3.608, 4.189] | 2.910 [2.903, 2.918] / 3.800 [3.482, 4.191] | −4.1%、−29.9%、−30.6% |
| Write / 4096 B | 5.043 [4.822, 5.256] / 2.912 [2.897, 2.929] | 5.925 [5.117, 6.769] / 2.808 [2.773, 2.853] | 3.464 [3.172, 3.767] / 3.357 [3.122, 3.626] | +42.3%、+52.6%、+3.1% |

64-byte writer 在两轮重复中 direct 都慢约 30%，属于通用单-buffer快路径不可接受的回归。4096-byte writer 初始优势没有在重复二中复现。reader 未持续达到 5% 门槛。没有加入候选实现或生产回归测试。

## 测量限制

测量期间主机有其他 Cargo test/check 和一个独立 benchmark 运行。Criterion 个别 case 的离群值达到每 100 个样本 20–23 个；重复二中不少 case 有 10–22% 离群值。代码未变时不同轮结果仍相差数十个百分点。64-byte writer 的 direct 路径重复回归，而 4096-byte writer 差值在第二次重复中大幅缩小。因此不声称存在性能提升，也不把这些热缓存结果外推到存储设备吞吐。

## RLF-001、RLF-003 与集成验证

- **RLF-001：** `observations_match` 只有双方 identity 均为 `Some` 且相等时才继续比较；缺失任一 identity 都返回 false。边界回归覆盖相等、缺失／不同 identity、native details、长度、count、Unix 修改／状态时间及 Windows 写入／状态时间。旧实现下缺失 identity 断言按预期失败；随后 `cargo test --locked --all-features --lib lf3_read_` 通过（10 passed）。
- **RLF-003：** 补齐向量 descriptor 转换、observation 查询和 reader 向量读取的 Rustdoc；新增公共 reader 示例。`cargo test --locked --all-features --doc local_file_reader` 通过（2 个 compile 示例、1 个 compile-fail 示例）。
- **Optional native query 语义对齐：** 设计要求普通 native I/O 错误不得被 Optional 吞掉。Unix Optional 对 `fstat` EIO 现在传播原 Io error/source；Windows Optional 只对明确 unsupported 的错误降级，AccessDenied 等普通错误传播；Required 的错误分类和 source 保持。TDD 下 Unix 新断言在旧实现失败、修复后 `cargo test --locked --all-features --lib lf3_native_unix` 通过（4 passed）。`cargo check --locked --all-features --target x86_64-pc-windows-gnu --lib` 通过。Windows `--tests` 编译因缺少 `x86_64-w64-mingw32-gcc` 未运行，实际 Windows test 仍须 CI 执行。
- **格式与完整门禁：** `/data/working/qubit/rust-common/.worktrees/rs-local-files/codex-rlf-20261104-implementation/.infra/bin/align-ci.sh` 退出码 0；相同路径下 `ci-check.sh` 退出码 0，覆盖 style、lockfile、verify、docs、readme、ASan、feature matrix、fuzz、package、coverage、audit 和平台任务。Coverage thresholds：functions 95.04%（3390/3567，门槛 ≥95%）、lines 92.11%（32991/35817，>90%）、regions 91.88%（51226/55752，>85%）。
- **下游：** 三个下游隔离 worktree 的 Cargo 解析均经 `cargo tree --locked -i qubit-local-files` 确认指向本轮集成 worktree。`rs-fs-local` 的 `cargo test --locked --lib` 通过（32 passed）；`rs-mime` 的 `temp_staging` 列表检查确认 4 项实际匹配，随后 `cargo test --locked --lib temp_staging` 通过（4 passed）；`rs-magika` 的 `cargo check --locked --all-targets` 通过。
- **平台缺口：** 本地平台矩阵的 Linux default、all-features、digest、lifecycle-model 和 Android ARMv7 compile-check suites 均通过。工具明确将 Windows suites 标为 `WARN+SKIP`（未配置 Windows VM/SSH guest runner）、macOS suites 标为 `WARN+SKIP`（当前主机为 Linux）、Android native 标为 `WARN+SKIP`（未配置 SDK/NDK 与设备）。跨目标 Windows library check 不是 Windows runtime test，不能替代实际 Windows CI。
