# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

`qubit-local-files` 是面向原生本地文件系统的策略化 API，适合不满足于零散
`std::fs` 调用的应用：它提供结构化错误上下文、明确的发布结果、惰性遍历、临时资源
所有权，以及以已打开目录为基础的访问权限。它直接使用原生 `Path` 和 `OsStr`，不依赖
`qubit-fs`；provider 适配由
[`qubit-fs-local`](https://crates.io/crates/qubit-fs-local) 提供。

## 安装

```toml
[dependencies]
qubit-local-files = "0.8"
```

## 快速开始：发布生成文件

构建工具和导出程序通常只有在全部字节写完后才应替换输出文件。下面的示例创建工作目录，
通过 writer 写入清单并提交，然后读回已经发布的内容。

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    create_temp_directory, open_reader, open_writer, LocalReadOptions,
    LocalTempDirectoryOptions, LocalWriteMode, LocalWriteOptions, LocalWriterState,
};

let work = create_temp_directory(&LocalTempDirectoryOptions::new())?;
let path = work.path().join("manifest.json");
let mut writer = open_writer(
    &path,
    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
)?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(outcome.state(), LocalWriterState::Committed);

let mut content = String::new();
open_reader(&path, &LocalReadOptions::new())?
    .read_to_string(&mut content)?;
assert_eq!(content, r#"{"version":1}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 提供的能力

| API | 适用场景 |
| --- | --- |
| Host 便利函数 | 直接执行主机范围的元数据、I/O、复制、重命名、遍历和临时条目操作。 |
| `LocalFileSystem::host()` | 可复用的进程可见主机命名空间服务。 |
| `LocalFileSystem::rooted(root)` | 以相同操作访问一个已打开目录权限下的后代。 |
| `LocalFileSystemScope` | 标识实例按主机路径还是 rooted 后代路径解释输入。 |
| `LocalFileWriter` | 分阶段发布后的显式提交或中止。 |
| `LocalDirectoryWalker` | 采用创建时固定策略的惰性目录枚举。 |
| `LocalTempFile` / `LocalTempDirectory` | 拥有清理责任，并支持 `keep` 与持久化。 |
| `LocalFileNames` / `LocalPaths` | 不丢失 UTF-8 以外文件名信息的原生文件名和词法路径工具。 |

普通应用可直接使用 Host 便利函数。需要保存配置、传递给其他组件或适配到更高层文件系统
SPI 时，则使用接口一致的 `LocalFileSystem` 实例。

符号链接策略按 `LocalFileSystem` 实例配置；Rooted 默认
`FollowWithinScope`，Host 默认 `FollowAcrossScope`。各类操作对最终链接的
具体语义请参阅[用户手册](doc/user_guide.zh_CN.md)。

临时资源清理会校验所有权，但它不是并发同步边界。guard 删除前会比较创建时保存的
文件系统标识，因此通常能拒绝误删替换条目；但标识检查与按路径删除是两个独立操作，
文件系统也可能复用标识。例如，不受信任的并发者删除临时文件后，反复在同一名称上安装新文件，
这不在清理契约的保证范围内。应将临时资源放在并发者无写权的目录，或调用 `keep` 后由上层协调删除。
每个临时资源都创建在独立的私有 sandbox 中，返回路径包含一个生成的 sandbox 组件；调用
`keep` 会把 sandbox 与资源一起交给调用方。

## 选择合适的权限范围

主机路径可使用便利函数或 `LocalFileSystem::host()`。当一个已打开目录就是权限边界时，
使用 `LocalFileSystem::rooted(root)`。两种实例提供相同操作，只改变路径解释方式。rooted
路径必须是相对后代；绝对路径、平台前缀、`.` 和 `..` 会被拒绝。中间符号链接遵循实例
策略：默认的 `FollowWithinScope` 保持在已打开 root 内，`FollowAcrossScope` 则显式允许越出。
之后重命名诊断用的根路径也不会重定向已打开的权限。

复制会根据源元数据选择文件或目录行为。复制和重命名失败会保留已证实的最强发布状态，
因此调用方必须检查类型化失败，不能假设出错后目标未变。`CreateNew` 和
`CreateOrReplace` 在目标目录中暂存；`Append` 会直接写入已有普通文件，不能满足要求的
原子性。

## 延伸阅读

- [User guide](doc/user_guide.md)
- [用户手册](doc/user_guide.zh_CN.md)
- [API 文档](https://docs.rs/qubit-local-files)
- [本地文件系统设计文档](doc/local_file_system_design.zh_CN.md)
- [English README](README.md)

## 平台范围

Linux、Windows 和 macOS 的行为会在运行时测试。FreeBSD 和 Android 仅编译检查配置路径；
本 crate 不承诺这些目标上的运行时保证。能力快照区分“构建已实现”和“运行时已验证”，不会探测具体的运行时文件系统；
在活动挂载点经过运行时验证前，不会向上层宣称 rename/copy 的耐久性；
无法满足要求的原子性或耐久性时，会在命名空间变更前拒绝操作。

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./ci-check.sh

# 检查代码覆盖率
./coverage.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./align-ci.sh`格式化代码，运行`./ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
