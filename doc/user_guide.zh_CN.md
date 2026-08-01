# Qubit Local Files 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-local-files)

本手册面向 Rust 1.94 及以上版本的 `qubit-local-files` 0.8 使用者，适用于直接操作主机
文件系统，或需要把操作限制在一个已打开目录之下的应用。它不是 provider 注册表、远程
文件系统 API，也不替代 provider 层的逻辑路径模型。

## 概念模型

```
主机路径 ── LocalFileSystem ── 原生文件系统
已打开根目录 ─ RootedLocalFileSystem ─ 仅相对后代路径
```

`LocalFileSystem` 以关联方法提供主机范围操作。`RootedLocalFileSystem` 通过 `open`
创建，是持有已打开根目录的有状态权限，而不是反复解析字符串路径。reader、writer、
walker 与临时条目都是拥有资源的有状态对象。`LocalFileNames` 和 `LocalPaths` 提供原生
词法工具，不会把文件名强制转换为 UTF-8。

## 场景：写入并检查导出产物

导出程序需要创建 `build/output`，只在完整写入后发布 `manifest.json`，并读回结果。成功
的可观察条件是 writer 返回 `Committed`，且能读取已经发布的字节。

```rust
use std::io::{Read, Write};
use qubit_local_files::{
    LocalCreateDirectoryOptions, LocalFileSystem, LocalReadOptions,
    LocalWriteMode, LocalWriteOptions, LocalWriterState,
};

let output = std::path::Path::new("build/output");
LocalFileSystem::create_directory(
    output,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;
let path = output.join("manifest.json");
let mut writer = LocalFileSystem::open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?
    .read_to_string(&mut text)?;
assert_eq!(text, r#"{"complete":true}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

多次系统调用使用的相对路径会在操作开始时绑定。复制和重命名会用同一个当前目录快照绑定
源和目标。`metadata` 观察最终条目本身，不跟随最终符号链接。

## 发布、复制与恢复

`CreateNew` 与 `CreateOrReplace` 使用目标目录内的暂存；`Append` 直接修改已有普通文件，
因此拒绝要求的原子性。writer 的生命周期（`LocalWriterState`）与发布结论
（`LocalWriteFailureState`）分离；写入、刷新或提交失败可能使发布状态变为不确定，需要
恢复时应保留并检查返回的资源或错误。

`copy` 根据源元数据选择文件或目录行为。需要固定源类型时使用 `with_file_source()` 或
`with_tree_source()`，并通过 `source_mode()` 读取模式。其选项分别控制目标冲突、类型冲突、
元数据、符号链接、设备边界、原子性和耐久性。无法满足的要求保证会在破坏性变更前被拒绝。
自复制和硬链接别名会被拒绝；覆盖符号链接目标时会替换该条目而不跟随它。

```rust,no_run
use qubit_local_files::{LocalCopyFailureState, LocalCopyOptions, LocalFileSystem};

match LocalFileSystem::copy(
    std::path::Path::new("source"),
    std::path::Path::new("backup"),
    &LocalCopyOptions::new(),
) {
    Ok(outcome) => println!("已复制 {} 个文件", outcome.stats().files()),
    Err(failure) => match failure.state() {
        LocalCopyFailureState::Unchanged => println!("目标未改变"),
        LocalCopyFailureState::PartiallyPublished => println!("目标部分发布"),
        LocalCopyFailureState::Published => println!("目标已发布"),
        LocalCopyFailureState::Indeterminate => println!("需要核对目标状态"),
    },
}
```

重命名也会通过类型化失败状态报告 `Unchanged`、`Renamed` 或 `Indeterminate`；出错并不等于
“什么都没发生”。

## 遍历和临时资源

`LocalFileSystem::list` 返回惰性的 `LocalDirectoryWalker`。它按需打开和推进目录；最大
深度与符号链接策略在创建时固定，drop 只释放句柄。

临时文件和目录在仍处于 armed 状态时拥有清理责任。drop 会尽力清理；`keep` 会关闭清理并
返回稳定的绝对路径。持久化失败会保留资源，调用方可重试、检查、保留或显式清理。创建前会
校验前缀和后缀：原生分隔符、NUL 与便携保留名称不会留下条目。

## Rooted 工作区

处理工作区下不受信任的相对名称时，应使用 rooted 访问。

```rust
use qubit_local_files::{LocalListOptions, RootedLocalFileSystem};

let root = RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let walker = root.list(std::path::Path::new("assets"), &LocalListOptions::new())?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

rooted 路径必须是相对后代。绝对路径、平台前缀、`.`、`..` 和中间符号链接都会被拒绝。诊断
用根路径不是权限本身：`open` 之后重命名它不会重定向已打开的资源。词法包含关系可用于早期
分类，但不能替代基于描述符的权限控制。

## 错误、诊断与排障

`LocalFileError` 包含 `LocalFileErrorKind`、`LocalFileOperation`、可用时的主/目标原生
路径，以及可选的 `std::io::Error` 来源。发布操作用专门失败类型保存部分成功状态。

`LocalPersistError` 同时保留临时资源和结构化的 `LocalFileError`；其 `state()` 是唯一的
恢复状态来源。存在原生 I/O 错误时，可从结构化错误的 source 取得。

| 症状 | 检查方式 |
| --- | --- |
| rooted 操作拒绝路径 | 传入相对后代，移除绝对前缀、`.`、`..` 与中间符号链接。 |
| 要求保证被拒绝 | 检查所选文件系统的 capability；仅在业务允许时放宽要求。 |
| copy 或 rename 出错 | 先检查类型化失败状态，再决定重试、清理或认定目标不存在。 |
| 临时条目仍存在 | 保留资源并调用显式生命周期方法；drop 清理只是尽力而为。 |

## 平台限制与延伸阅读

Linux、Windows 和 macOS 会进行运行时测试。FreeBSD 与 Android 仅做编译检查。
`LocalFileSystem::capabilities()` 报告主机实现；`RootedLocalFileSystem::capabilities()`
返回打开权限时缓存的快照。路径限制只有对目标文件系统验证成功时才是 `Some`。
原子 rename、原子 replace 与临时资源原子持久化会分别报告，因为各平台支持并不相同。

继续阅读 [README](../README.zh_CN.md)、[English user guide](user_guide.md) 或
[API 文档](https://docs.rs/qubit-local-files)。
