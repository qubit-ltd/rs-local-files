# Qubit Local Files

[![Rust CI](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml/badge.svg)](https://github.com/qubit-ltd/rs-local-files/actions/workflows/ci.yml)
[![Coverage](https://img.shields.io/endpoint?url=https://qubit-ltd.github.io/rs-local-files/coverage-badge.json)](https://qubit-ltd.github.io/rs-local-files/coverage/)
[![Crates.io](https://img.shields.io/crates/v/qubit-local-files.svg?color=blue)](https://crates.io/crates/qubit-local-files)
[![Rust](https://img.shields.io/badge/rust-1.94+-blue.svg?logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![English Document](https://img.shields.io/badge/Document-English-blue.svg)](README.md)

面向 Rust 的本地文件系统工具库。

## 概述

Qubit Local Files 承载从 `qubit-io` 拆出的本地文件系统工具。它专注于具体本地路径和本地文件系统条目：临时文件和目录、文件名 helper、递归目录操作，以及持久化同目录 atomic write。

适合使用本 crate 的场景包括：

- 需要 drop 时自动清理的 RAII 临时文件或临时目录；
- 打开或写入本地文件前需要自动创建父目录；
- 需要递归清理目录、计算目录大小或复制目录树；
- 需要默认拒绝意外覆盖的保守复制和持久化行为；
- 需要随机、portable 或 lexical 文件名 helper；
- 需要持久化替换写入，使读取方只能观察到旧完整文件或新完整文件。

详细用法、示例和 API 选择建议请参见[中文用户手册](doc/user_guide.zh_CN.md)。API 参考文档可在 [docs.rs](https://docs.rs/qubit-local-files) 查看。

如果需要 stream 层 `std::io` trait、extension method、wrapper 和 codec，请参考
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 安装

```toml
[dependencies]
qubit-local-files = "0.1"
```

## 快速示例

```rust
use std::io::Write;

use qubit_local_files::{
    FileWriteMode,
    FileWriteOptions,
    LocalCopyDirOptions,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};

let work = LocalTempDir::with_prefix(Some("qubit-local-files-readme-"))?;
let src = work.path().join("src");
let dst = work.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("manifest.json"), br#"{"version":1}"#)?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;
assert_eq!(1, stats.files);

LocalFiles::atomic_write(dst.join("manifest.json"), br#"{"version":2}"#)?;

let final_path = work.path().join("result.txt");
std::fs::write(&final_path, "old payload")?;

let mut temp = LocalTempFile::with_name(Some("qubit-local-files-"), Some(".txt"))?;
temp.writer(FileWriteOptions::new(FileWriteMode::CreateOrTruncate).buffered())?
    .write_all(b"new payload\n")?;
temp.persist_with(&final_path, LocalPersistOptions { overwrite: true })?;

assert_eq!("new payload\n", std::fs::read_to_string(&final_path)?);

# Ok::<(), std::io::Error>(())
```

## 主要能力

### LocalFiles 命名空间

`LocalFiles` 集中提供容易在业务代码中反复出现的小型本地文件系统操作：

| 方法 | 用途 |
| --- | --- |
| `exists` | 以 `std::io::Result<bool>` 检查路径是否存在，不把检查错误静默折叠成 `false`。 |
| `metadata` | 读取本地路径 metadata。 |
| `list` | 列出目录直接子项。 |
| `open_reader` | 使用 `FileReadOptions` 打开 `LocalFileReader`。 |
| `open_writer` | 使用 `FileWriteOptions` 打开 `LocalFileWriter`。 |
| `ensure_dir` | 创建目录及缺失祖先目录。 |
| `ensure_parent` | 为文件路径创建缺失父目录。 |
| `dir_size` | 统计目录下普通文件的总字节数，不跟随 symbolic link。 |
| `clean_dir` | 删除目录中的所有子项，但保留目录本身。 |
| `remove_any` | 删除文件、目录树或 symbolic link。 |
| `copy_dir_all_with` | 使用显式选项递归复制本地目录树，并返回统计信息。 |
| `atomic_write` | 通过持久化同目录临时写入替换文件。 |
| `atomic_write_with` | 与 `atomic_write` 相同，但由调用方提供写入逻辑。 |

### 临时文件和临时目录

`LocalTempFile` 和 `LocalTempDir` 创建真实的本地文件系统条目，并在 drop 时自动删除，除非通过 `keep` 或 `persist` 释放所有权。Drop 阶段的清理是 best-effort；失败会通过 `log` 门面以 `warn!` 记录告警，不会 panic。

`LocalTempFile` 面向写入场景：通过 `writer(FileWriteOptions)` 配置内部 writer，通过 `close` flush 并关闭后，再用其他 API 读取该路径。它有意不提供读取 helper；确实需要读取时，通过 `LocalFiles` 或 `std::fs` 操作它的路径。

`LocalTempDir` 提供安全 child helper：`child_path`、`ensure_child_dir`、`open_child_reader`、`open_child_writer` 和 `list`。child 路径必须是相对路径，不能包含父目录跳转。`ensure_child_dir` 会像 `mkdir -p` 一样创建多层缺失父目录。

`LocalTempFile::persist` 默认在移动操作中拒绝已存在的目标。只有确实要替换已有目标时，才使用 `LocalTempFile::persist_with` 和 `LocalPersistOptions { overwrite: true }`。`LocalTempDir::persist` 同样拒绝已存在的目标，并且不提供 overwrite 选项。

### 读写选项

普通文件打开操作有意保持显式：

| 类型 | 用途 |
| --- | --- |
| `FileReadOptions` | 控制 reader 是否缓冲。 |
| `FileWriteOptions` | 控制是否创建父目录、写入模式和 writer 是否缓冲。 |
| `FileBuffering` | 选择无额外缓冲，或使用可选容量的缓冲 I/O。 |
| `FileWriteMode` | 选择 `OpenExistingAtStart`、`CreateNew`、`CreateOrTruncate`、`AppendExisting` 或 `AppendOrCreate`。 |

`atomic_write` 仍然是独立 API，因为它执行的是完整替换协议，而不是普通写句柄打开。

### Atomic Write

`LocalFiles::atomic_write` 会在同一父目录下写入临时文件，flush 并 sync 这个临时文件，替换目标，并在支持的平台上 sync 父目录。它适合配置文件、cache manifest、checkpoint、生成索引等 whole-file replacement 场景。

该操作不是多文件事务，也不协调并发写入。如果多个进程或线程可能同时替换同一路径，需要使用外部锁。

### 递归目录复制

`LocalFiles::copy_dir_all_with` 复制目录树并返回 `LocalCopyDirStats`：

| 字段 | 含义 |
| --- | --- |
| `files` | 已复制的普通文件数量。 |
| `directories` | 已创建的目标目录数量。 |
| `bytes` | 从普通文件复制的字节数。 |

`LocalCopyDirOptions::default()` 是有意保守的默认值：不覆盖已存在的目标条目，不跟随 symbolic link，也不保留源权限。需要这些行为时，应显式设置 `overwrite`、`follow_symlinks` 或 `preserve_permissions`。

### 文件名 Helper

`LocalFilenames` 提供随机和 lexical 文件名工具：

| 方法组 | 用途 |
| --- | --- |
| `random`、`random_with` | 构造随机文件名 component，生成失败时 panic。 |
| `try_random`、`try_random_with` | 通过 `std::io::Result` 构造随机文件名 component。 |
| `validate_portable_file_name` | 校验保守 portable 的单 component 文件名。 |
| `file_name`、`file_stem`、`file_prefix` | 按 `Path` 语义提取 UTF-8 path component。 |
| `extension`、`dot_extension`、`has_extension` | 检查最终扩展名。 |
| `has_extension_ignore_ascii_case` | 使用 ASCII-only 大小写折叠检查最终扩展名。 |
| `file_name_from_path` | 从 path-like 字符串中提取最后一段。 |
| `file_name_from_url` | 提取 URL 最后一个 path segment，并解码安全的 percent-encoded UTF-8。 |

这些 lexical helper 不访问文件系统。返回文件名数据的公开方法返回 UTF-8 字符串，而不是 `OsStr`；无效 UTF-8 path component 返回 `None`。

## Crate 边界

`qubit-local-files` 有意只覆盖本地文件系统相关能力。它不提供：

- stream extension trait、binary codec 或 stream wrapper；
- 异步文件系统 API 或 runtime 集成；
- 远程文件系统、FTP、S3、对象存储或 VFS 抽象；
- file watching、globbing 或通用目录遍历框架；
- 锁或跨进程写入协调。

stream 和字节 I/O 相关能力请使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 运行时依赖

本 crate 运行时依赖 Rust 标准库、`getrandom`、`libc` 和 `log`。`getrandom` 用于生成随机临时名，`libc` 用于 Linux no-replace rename 支持，`log` 用于 drop 阶段的清理失败告警。

## 测试与代码覆盖率

本项目为临时文件和目录清理、覆盖行为、atomic write、递归复制行为、文件名 helper 和公开文件系统工具保持测试覆盖。

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行覆盖率报告
./coverage.sh

# 生成文本格式报告
./coverage.sh text

# 运行 CI 检查（格式化、clippy、测试、覆盖率、audit）
./ci-check.sh
```

## 许可证

Copyright (c) 2026. Haixing Hu.

根据 Apache 许可证 2.0 版（"许可证"）授权；
除非遵守许可证，否则您不得使用此文件。
您可以在以下位置获取许可证副本：

    http://www.apache.org/licenses/LICENSE-2.0

除非适用法律要求或书面同意，否则根据许可证分发的软件
按"原样"分发，不附带任何明示或暗示的担保或条件。
有关许可证下的特定语言管理权限和限制，请参阅许可证。

完整的许可证文本请参阅 [LICENSE](LICENSE)。

## 贡献

欢迎贡献。请随时提交 Pull Request。

### 开发指南

- 遵循 Rust API 指南。
- 将本地文件系统相关能力保留在 `qubit-local-files` 中。
- stream 和字节 I/O 工具请使用 [qubit-io](https://github.com/qubit-ltd/rs-io)。
- 可能覆盖数据或离开请求源目录的操作，应保持保守默认值。
- 为平台相关文件系统行为保持全面测试覆盖。
- 公共 API 在有助于说明行为时应提供文档和示例。
- 提交 PR 前确保 `./ci-check.sh` 通过。

## 作者

**Haixing Hu**

## 相关项目

- [qubit-io](https://github.com/qubit-ltd/rs-io)：面向 Rust 的 stream 和字节 I/O 工具库。
- Qubit 旗下的更多 Rust 库发布在 GitHub 组织 [qubit-ltd](https://github.com/qubit-ltd)。

---

仓库地址：[https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
