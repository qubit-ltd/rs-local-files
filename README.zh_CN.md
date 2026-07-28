# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English](https://img.shields.io/badge/docs-English-blue.svg)](README.md)

面向 Rust 的统一 native 本地文件系统操作库。

## 概览

Qubit Local Files 通过同一套策略化 API 提供 host-wide 与 descriptor-anchored
本地文件系统操作。它直接接受 native `Path` 和 `OsStr`，保留平台文件名，并把
containment、publication 及平台差异封装在本 crate 内部。

主要能力包括：

- 结构化本地文件系统错误和结果；
- 文件与目录统一复制；
- 惰性递归遍历；
- staged publication 与显式 append 语义；
- RAII 临时文件和临时目录；
- descriptor/handle-relative rooted authority；
- native 路径和文件名校验。

本 crate 不依赖 `qubit-fs`。Provider-neutral 转换应由 `qubit-fs-local` 完成。

完整使用说明见[用户手册](doc/user_guide.zh_CN.md)，整体契约见
[设计文档](doc/local_file_system_design.zh_CN.md)。

## 安装

```toml
[dependencies]
qubit-local-files = "0.7"
```

## 快速示例

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalFileSystem,
    LocalReadOptions,
    LocalTempDirectoryOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
};

let work =
    LocalFileSystem::create_temp_directory(&LocalTempDirectoryOptions::new())?;
let path = work.path().join("state.json");

let mut writer =
    LocalFileSystem::open_writer(
        &path,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )?;
writer.write_all(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(LocalWriterState::Committed, outcome.state());

let mut reader =
    LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?;
let mut content = String::new();
reader.read_to_string(&mut content)?;
assert_eq!(r#"{"version":1}"#, content);

# Ok::<(), Box<dyn std::error::Error>>(())
```

## 主要类型

| 类型 | 用途 |
| --- | --- |
| `LocalFileSystem` | Host metadata、读写、复制、遍历、创建、删除、rename 和临时资源。 |
| `RootedLocalFileSystem` | 一个已打开 root 下的 descriptor/handle-relative authority。 |
| `LocalFileNames` / `LocalPaths` | 不进行 lossy UTF-8 转换的 native lexical 工具。 |
| `LocalDirectoryWalker` | 固定 depth 与 symlink 策略的惰性遍历器。 |
| `LocalFileReader` / `LocalFileWriter` | 拥有 I/O 资源并显式报告 publication 状态。 |
| `LocalTempFile` / `LocalTempDirectory` | 拥有 cleanup responsibility 的临时条目。 |
| `LocalFileError` | 稳定错误分类、操作与 native path 上下文。 |

所有无状态操作通过关联方法组织；旧的 public free-function namespace 不再属于公共 API。

## Rooted 访问

```rust
use std::io::Read;

use qubit_local_files::{
    LocalReadOptions,
    RootedLocalFileSystem,
};

let root = RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let mut reader =
    root.open_reader(std::path::Path::new("config/app.toml"), &LocalReadOptions::new())?;
let mut content = String::new();
reader.read_to_string(&mut content)?;

# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted 操作拒绝 absolute path、平台 prefix、`.` 与 `..`，并从已打开 root authority
派生 descendant access，而不依赖之后再次查找诊断路径字符串。

## 平台支持

| 支持级别 | 目标 | 验证 |
| --- | --- | --- |
| Runtime-tested | Linux、Windows、macOS | CI 执行平台文件系统测试。 |
| Compile-only | FreeBSD、Android | 检查 production cfg path，不宣称 runtime 保证。 |

Capability snapshot 只报告当前实现能够提供的保证。无法满足 required atomicity 或
durability 时，会在 namespace 变更前拒绝操作。

## Runtime 依赖

本 crate 使用 Rust 标准库、`getrandom`、`libc`、`log` 以及 target-specific
`windows-sys` binding。

## 测试

```bash
cargo test --all-features
./align-ci.sh
./ci-check.sh
```

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目采用 Apache License 2.0，完整文本见 [LICENSE](LICENSE)。

## 贡献

欢迎贡献。请同步维护公共文档和平台行为测试，并运行 `./align-ci.sh` 与
`./ci-check.sh`。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库：[https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
