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
qubit-local-files = "0.3"
```

## 快速开始：发布生成文件

构建工具和导出程序通常只有在全部字节写完后才应替换输出文件。下面的示例创建工作目录，
通过 writer 写入清单并提交，然后读回已经发布的内容。

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalFileSystem, LocalWriteMode, LocalWriteOptions, LocalWriterState,
};

let mut filesystem = LocalFileSystem::host()?;
filesystem.set_default_write_options(LocalWriteOptions::new(
    LocalWriteMode::CreateOrReplace,
))?;

let work = filesystem.create_temp_directory()?;
let path = work.path().join("manifest.json");
let mut writer = filesystem.open_writer(&path)?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(outcome.state(), LocalWriterState::Committed);

let mut content = String::new();
filesystem.open_reader(&path)?
    .read_to_string(&mut content)?;
assert_eq!(content, r#"{"version":1}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 提供的能力

| API | 适用场景 |
| --- | --- |
| `LocalFileSystem::host()` | 可复用的进程可见主机命名空间服务。 |
| `LocalFileSystem::rooted(root)` | 以相同操作访问一个已打开目录权限下的后代。 |
| `LocalFileSystemScope` | 标识实例按主机路径还是 rooted 后代路径解释输入。 |
| `LocalFileWriter` | 分阶段发布后的显式提交或中止。 |
| `LocalDirectoryWalker` | 采用创建时固定策略的惰性目录枚举。 |
| `LocalTempFile` / `LocalTempDirectory` | 拥有清理责任，并支持 `keep` 与持久化。 |
| `LocalFileNames` / `LocalPaths` | 不丢失 UTF-8 以外文件名信息的原生文件名和词法路径工具。 |

`LocalFileSystem` 是有状态的实例 API。每个实例拥有自己的当前目录、符号链接策略和九种
操作的默认 Options。普通方法使用实例默认值；每个 `*_with_options` 方法都以传入的完整
Options 替代实例默认值。clone 会复制全部可变状态，Rooted clone 只共享不可变的已打开
authority。本 crate 不承诺共享可变配置时的线程安全；调用方可以每线程持有一个 clone，
也可以自行添加同步包装。

资源预算均由调用方选择。遍历和复制的深度、条目数、字节数、打开目录数、deadline、
重复名称内存、打开重试时间以及临时名称尝试次数，在调用方显式设置前都不会形成库内隐藏上限。

符号链接策略按 `LocalFileSystem` 实例配置；Rooted 默认
`FollowWithinScope`，Host 默认 `FollowAcrossScope`。Rooted 仅支持
`Reject` 和 `FollowWithinScope`；选择 `FollowAcrossScope` 会返回
`InvalidOptions`。各类操作对最终链接的具体语义请参阅[用户手册](doc/user_guide.zh_CN.md)。

临时资源清理会校验所有权，但它不是并发同步边界。调用方需要观察清理失败时应显式调用
`cleanup()`；drop 只提供尽力而为的兜底。guard 删除前会比较创建时保存的
文件系统标识，因此通常能拒绝误删替换条目；但标识检查与按路径删除是两个独立操作，
文件系统也可能复用标识。例如，不受信任的并发者删除临时文件后，反复在同一名称上安装新文件，
这不在清理契约的保证范围内。应将临时资源放在并发者无写权的目录，或调用 `keep` 后由上层协调删除。
每个临时资源都创建在独立的私有 sandbox 中，返回路径包含一个生成的 sandbox 组件；调用
`keep` 会把 sandbox 与资源一起交给调用方。

## 选择合适的权限范围

主机路径使用 `LocalFileSystem::host()`。当一个已打开目录就是权限边界时，
使用 `LocalFileSystem::rooted(root)`。两种实例提供相同操作，只改变路径解释方式。rooted
和 Host 实例都拥有自己的 namespace-absolute PWD。相对路径从该 PWD 开始；`.` 与空路径
表示 PWD；`..` 会逐层规范化，只在试图越过 namespace root 时被拒绝。在 Rooted 实例中，
`/etc/hosts` 是已打开 root 下的虚拟绝对路径，而不是 Host 的 `/etc/hosts`；native prefix
会被拒绝。

中间符号链接遵循实例策略。Rooted 的绝对链接目标会从虚拟 `/` 重新开始解析，默认
`FollowWithinScope` 会阻止任何链接越出已打开的 authority。`FollowAcrossScope` 仅适用于
Host，Rooted 会拒绝该配置。之后重命名诊断用 root 路径也不会重定向已打开的 authority。
公共资源路径和错误路径统一使用可再次传入同一实例的 namespace-absolute 身份；底层物理
路径仅在可获得时作为可选诊断信息提供。

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
本 crate 不承诺这些目标上的运行时保证。能力快照只报告当前 target 是否实现了对应的完整操作协议，
不会探测具体的运行时文件系统，也不声称证明底层硬件已经持久化数据；无法满足要求的原子性或耐久性时，
会在命名空间变更前拒绝操作。
Windows Host 路径转换明确不支持 UNC 路径。

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
