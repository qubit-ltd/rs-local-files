# RLF4-Q01 Windows 暂存清理证据

## 结论

**状态：BLOCKED_NATIVE。** 当前执行环境为 Linux x86_64，不能观测 Windows Rooted staging handle 的改名、按名清理及 Windows 文件系统共享语义。本记录不把 Linux 测试、交叉编译或 Windows-gated 测试的零执行数作为 Q01 结论，也不修改生产行为。

## 本机环境与代码基线

| 字段 | 已核实事实 |
| --- | --- |
| OS / 架构 | Linux `7.0.0-38-generic`，x86_64（`uname -a`） |
| 仓库所在文件系统 | `/data` 为 ext4（`df -T`）；不是 Windows 本地文件系统 |
| 执行用户 | uid 1000，`starfish`；该权限不能代表 Windows 用户权限 |
| Rust | `rustc 1.94.0 (4a4ef493e 2026-03-02)` |
| 起始 HEAD | `91e1ccaf51a42a2d79eb60730640f43b5ecd6669` |
| 新增回归用例 | 3 个 Windows-gated 用例，列于下方；尚未在 Windows 执行 |
| 默认 feature 执行数 | 0（未执行；Linux 不能运行 Windows 测试） |
| `--all-features` 执行数 | 0（未执行；Linux 不能运行 Windows 测试） |

## 回归用例

每个用例独立创建 fixture，先写 sentinel，通过打开 writer 前后的目录条目差集唯一识别 staging 名。测试在 writer 持有期间将 staging 改名，在旧名写入竞争字节，然后检查竞争文件名和字节、sentinel，以及移动后的原 staging 是安全清理还是保持原字节。

- `rooted_staging_abort_preserves_competitor_at_old_name`
- `rooted_staging_drop_preserves_competitor_at_old_name`
- `rooted_staging_failed_commit_cleanup_preserves_competitor_at_old_name`

失败提交场景先打开 `CreateNew` writer，再创建目标竞争文件，触发确定的 `AlreadyExists` 提交失败，随后对错误返回的 retained writer 执行 abort 清理。

## 未取得的 Windows 机制证据

- Windows OS 版本、架构、原生 Rust 工具链与完整 HEAD/features 执行日志。
- Windows 用户权限及实际卷类型（需确认是本地文件系统）。
- writer 持有 staging 句柄时，Windows 是否允许将 staging 改名；若不允许，需记录具体 fixture 阻塞，不能算作否证。
- abort、Drop、失败提交清理时旧名处竞争文件是否仍存在且字节不变，以及移动后的原 staging 是否被安全删除或保留。
- 默认与 `--all-features` 两种 Windows 原生配置下三个完整测试名的实际执行数和输出。

## 后续原生执行

在 Windows 本地文件系统上分别运行：

```text
cargo test --locked --test windows_rooted_staging_cleanup_tests -- --nocapture
cargo test --locked --all-features --test windows_rooted_staging_cleanup_tests -- --nocapture
```

只有确认三个场景均实际执行，且 fixture 的 staging 识别、改名、竞品创建均成功，才依据结果判定缺陷或否证。当前未运行上述命令；本任务保留新测试和生产实现现状。
