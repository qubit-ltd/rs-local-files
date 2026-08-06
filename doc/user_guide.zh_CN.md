# Qubit Local Files 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-local-files)

本手册面向 Rust 1.94 及以上版本的 `qubit-local-files` 0.3 使用者，适用于直接操作主机
文件系统，或需要把操作限制在一个已打开目录之下的应用。它不是 provider 注册表、远程
文件系统 API，也不替代 provider 层的逻辑路径模型。

## 概念模型

```
主机路径 ── LocalFileSystem::host()
已打开根目录 ─ LocalFileSystem::rooted(root) ─ 仅相对后代路径
```

`LocalFileSystem` 是主机范围的服务形式：`host()` 选择进程可见路径，
`rooted(root)` 打开目录权限并只接受相对后代。
两种形式提供相同操作，调用方和适配器无需分别面向 host 和 rooted 接口。reader、writer、
walker 与临时条目都是拥有资源的有状态对象。`LocalFileNames` 和 `LocalPaths` 提供原生
词法工具，不会把文件名强制转换为 UTF-8。

## 符号链接策略

`LocalFileSystem` 实例保存一个由所有操作继承的符号链接策略。
`LocalFileSystem::rooted(root)` 默认使用 `FollowWithinScope`：允许跟随链接，
但解析结果必须仍位于已打开的 root 内。Host 默认使用 `FollowAcrossScope`，因为
Host 没有更窄的 root 边界。可以通过 `with_symlink_policy` 选择 `Reject`、
`FollowWithinScope` 或 `FollowAcrossScope`；`list` 和 `copy` 的 options 可以
对单次操作覆盖策略。

策略作用于所有中间路径组件。Rooted 配置为 `FollowAcrossScope` 时，像
`etc/link/config` 这样的路径可以读写 `link` 指向的 root 外对象。这是显式授予的能力，
适合把 Git checkout 通过符号链接接入 `/etc` 的场景。

最终路径组件遵循真实文件系统中的操作语义：

| 操作 | 最终符号链接 |
| --- | --- |
| `metadata` | 查看链接条目本身。 |
| `open_reader` | Unix 跟随链接；Windows 拒绝最终 name-surrogate reparse point。 |
| `CreateNew` writer | 将已有链接视为已存在条目。 |
| `Append` writer | 跟随链接追加到目标。 |
| `CreateOrReplace` writer | 跟随链接替换目标，并保留链接。 |
| `delete` | 删除链接条目。 |
| `rename` | 移动或替换链接条目。 |
| `copy` 源 | 复制链接条目本身。 |
| `copy` 目标 | 替换目标链接条目。 |
| `temp persist` | 通过 rename 发布并替换目标链接条目。 |

目录遍历在有效策略允许时跟随目录链接。返回路径保持逻辑路径，例如 `link/child`，而不
是规范化后的目标路径；递归遍历按底层目录对象身份检测循环。深度限制按逻辑路径条目计算，
穿过链接不会额外增加一层。

## 场景：写入并检查导出产物

导出程序需要创建 `build/output`，只在完整写入后发布 `manifest.json`，并读回结果。成功
的可观察条件是 writer 返回 `Committed`，且能读取已经发布的字节。

```rust
use std::io::{Read, Write};
use qubit_local_files::{
    LocalCreateDirectoryOptions, LocalFileSystem, LocalReadOptions, LocalWriteMode,
    LocalWriteOptions, LocalWriterState,
};

let filesystem = LocalFileSystem::host();
let output = std::path::Path::new("build/output");
filesystem.create_directory(
    output,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;
let path = output.join("manifest.json");
let mut writer = filesystem.open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
filesystem.open_reader(&path, &LocalReadOptions::new())?
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

`LocalFileSystem::copy` 根据源元数据选择文件或目录行为。需要固定源类型时使用 `with_file_source()` 或
`with_tree_source()`，并通过 `source_mode()` 读取模式。其选项分别控制目标冲突、类型冲突、
元数据、符号链接、原子性和耐久性；复制策略不包含 mount 或 device 边界。无法满足的要求保证会在破坏性变更前被拒绝。
自复制和硬链接别名会被拒绝；覆盖符号链接目标时会替换该条目而不跟随它。

```rust,no_run
use qubit_local_files::{LocalCopyFailureState, LocalCopyOptions, LocalFileSystem};

match LocalFileSystem::host().copy(
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
深度、符号链接策略与句柄预算在创建时固定。默认 `Reopen` 策略会在达到预算时关闭并重新打开
活动 frame；显式选择 `Fail` 才会返回 `ResourceLimit`。零句柄预算无效并返回 `InvalidOptions`，
drop 只释放句柄。

临时文件和目录在仍处于 armed 状态时拥有清理责任。每个资源都创建在独立的私有 sandbox 中，
sandbox 会和资源一起清理。drop 会尽力清理；`keep` 会关闭清理并
返回 authority-local 路径（Host 为绝对路径，Rooted 为相对于已打开 root 的相对路径）。持久化失败会保留资源，调用方可重试、检查、保留或显式清理。创建前会
校验前缀和后缀：原生分隔符、NUL 与便携保留名称不会留下条目。

## Rooted 工作区

处理工作区下不受信任的相对名称时，应使用 rooted 访问。

```rust
use qubit_local_files::{LocalFileSystem, LocalListOptions};

let root = LocalFileSystem::rooted(std::path::Path::new("workspace"))?;
let walker = root.list(std::path::Path::new("assets"), &LocalListOptions::new())?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

walker 会按需打开并推进目录；最大深度、符号链接策略和默认 64 个目录句柄的预算会在创建时
固定。默认 `Reopen` 会在达到预算时关闭并重新打开活动 frame；显式选择 `Fail` 才返回
`ResourceLimit`。rooted walker 还会逐项读取目录，避免先将单个目录完整收集到 `Vec` 中。

rooted 路径必须是相对后代。绝对路径、平台前缀、`.` 和 `..` 会被拒绝；中间符号链接遵循
实例策略。`FollowWithinScope` 会拒绝解析到 root 外的链接，`FollowAcrossScope` 则允许该操作。
诊断用根路径不是权限本身：`open` 之后重命名它不会重定向仍采用描述符权限的操作。词法包含
关系可用于早期分类，但不能替代基于描述符的权限控制。

## 错误、诊断与排障

`LocalFileError` 包含 `LocalFileErrorKind`、`LocalFileOperation`、可用时的主/目标原生
路径，以及可选的 `std::io::Error` 来源。发布操作用专门失败类型保存部分成功状态。

`LocalPersistError` 同时保留临时资源和结构化的 `LocalFileError`；其 `state()` 是唯一的
恢复状态来源。存在原生 I/O 错误时，可从结构化错误的 source 取得。

| 症状 | 检查方式 |
| --- | --- |
| rooted 操作拒绝路径 | 传入相对后代，移除绝对前缀、`.`、`..`；若中间链接越界是有意行为，选择 `FollowAcrossScope`。 |
| 要求保证被拒绝 | 检查所选文件系统的 capability；仅在业务允许时放宽要求。 |
| copy 或 rename 出错 | 先检查类型化失败状态，再决定重试、清理或认定目标不存在。 |
| 临时条目仍存在 | 保留资源并调用显式生命周期方法；drop 清理只是尽力而为。 |

## 平台限制与延伸阅读

Linux、Windows 和 macOS 会进行运行时测试。FreeBSD 与 Android 仅做编译检查。
可通过 `LocalFileSystem::host().capabilities()` 查看主机实现；rooted 实例返回打开
权限时缓存的快照，`scope()` 供集成层区分两种命名空间；Rooted 实例的诊断锚点通过
`diagnostic_root()` 单独读取。Host 命名空间的 `limits()` 返回 `SizeLimit::VariesByPath`；使用
`limits_at(path)` 才会针对该路径所在文件系统返回有限值（无法探测时为
`Unknown`）。
原子 rename、原子 replace 与临时资源原子持久化会分别报告，因为各平台支持并不相同。

继续阅读 [README](../README.zh_CN.md)、[English user guide](user_guide.md) 或
[API 文档](https://docs.rs/qubit-local-files)。
