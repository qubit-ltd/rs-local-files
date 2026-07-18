# Qubit Local Files 用户手册

Qubit Local Files 是 Qubit Rust crate 家族中的本地文件系统 crate。它专注于具体本地路径、文件名、临时文件系统条目、递归目录操作，以及持久化同目录 atomic write。它有意不做 stream codec crate，也不做远程文件系统抽象。

如果需要 stream 层 `std::io` trait、extension method、wrapper 和 codec，请参考
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 何时使用本 crate

当代码处理的是本地文件系统路径，而不是 generic 字节流时，适合使用 `qubit-local-files`。典型场景包括 CLI 工具、代码生成器、cache writer、checkpoint 文件、本地导入导出任务、解包后的工作目录，以及需要临时本地文件的测试。

适合的场景：

- 创建通常应该自动清理的临时文件或临时目录。
- 成功完成工作后保留或持久化临时条目。
- 持久化临时文件时拒绝意外覆盖。
- 只有在显式指定 `LocalPersistOptions::new().with_overwrite()` 时才替换已有文件。
- 打开、写入或持久化文件前创建父目录。
- 通过同目录临时文件 atomic replacement 完整替换文件。
- 使用显式冲突和 symlink 策略复制本地目录树。
- 保留目录自身，只清理目录内容。
- 在不跟随 symbolic link 的前提下计算本地目录大小。
- 生成随机文件名 component 或校验 portable 文件名。

不适合的场景：

- 读取、写入、比较、限制或编码任意字节流。
- 实现 binary、LEB128、ZigZag 或 length-prefixed string codec。
- 用一个 API 抽象本地、FTP、对象存储或远程文件系统。
- 监听文件变化。
- 用锁协调并发写入。
- 提供绑定某个 runtime 的异步文件系统 API。

这些 stream 和字节 I/O 能力请使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 安装

```toml
[dependencies]
qubit-local-files = "0.7"
```

## 导入方式

从 crate root 导入具体命名空间、guard 和 option struct：

```rust
use qubit_local_files::{
    FileBuffering,
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyTypeConflictPolicy,
    LocalFilenames,
    LocalFiles,
    LocalPersistOptions,
    LocalTempDir,
    LocalTempFile,
};
```

本 crate 当前不暴露 prelude。显式导入可以让文件系统副作用和覆盖策略在调用点保持清晰。

## 读写选项

普通文件打开由显式 option struct 控制：

| 类型 | 字段 | 用途 |
| --- | --- | --- |
| `FileReadOptions` | `buffering` | 控制 `open_reader` 返回无额外缓冲 reader，还是 buffered reader。 |
| `FileWriteOptions` | `create_parent`、`mode`、`buffering` | 控制是否创建父目录、写入模式和 writer 是否缓冲。 |
| `FileBuffering` | `Unbuffered`、`Buffered { capacity }` | 选择原始文件 I/O，或带可选非零容量的 `BufReader` / `BufWriter`。 |
| `FileWriteMode` | enum variants | 选择目标文件的写入打开方式。 |

`LocalFiles::open_reader` 返回的 reader 实现 `Read` 和 `Seek`。
`LocalFiles::open_writer` 返回的 writer 实现 `Write` 和 `Seek`。
两个 helper 都只返回普通文件；目录、FIFO、socket 和其他特殊文件系统资源会被
拒绝，在 Unix 上拒绝 FIFO 时不会等待另一端连接。
`LocalFileWriter::sync_all` 和 `LocalFileWriter::sync_data` 会先 flush
缓冲内容，再同步底层文件，适合 append log 或其他不需要 whole-file atomic
replacement 的普通写句柄。对 writer 执行 seek 不会关闭 append-mode 语义。

`FileBuffering::buffered_with_capacity`、
`FileReadOptions::buffered_with_capacity` 和
`FileWriteOptions::buffered_with_capacity` 返回 `std::io::Result` 并拒绝零容量。
成功构造的自定义容量以 `NonZeroUsize` 保存，因此文件打开方法不会收到无效的零容量 policy。

写入模式：

| 模式 | 行为 |
| --- | --- |
| `OpenExistingAtStart` | 打开已存在文件，从 offset zero 开始写入，不截断。 |
| `CreateNew` | 创建新文件，目标已存在时报错。 |
| `CreateOrTruncate` | 目标缺失时创建，目标已存在时截断。这是默认值。 |
| `AppendExisting` | 追加到已存在文件，目标缺失时报错。 |
| `AppendOrCreate` | 追加到已存在文件，目标缺失时创建。 |

`LocalFiles::atomic_write` 有意不并入 `FileWriteOptions`。它执行的是完整的持久化替换协议，而不是返回普通写句柄。

## 临时目录

当临时目录通常应该自动清理时，使用 `LocalTempDir`。目录会立即创建，并在 guard drop 时递归删除。

```rust
use qubit_local_files::LocalTempDir;

let dir = LocalTempDir::with_prefix("qubit-local-files-work-")?;
std::fs::write(dir.path().join("scratch.txt"), b"scratch")?;

# Ok::<(), std::io::Error>(())
```

创建方法：

| 方法 | 用途 |
| --- | --- |
| `LocalTempDir::new` | 使用默认前缀在 `std::env::temp_dir()` 中创建临时目录。 |
| `LocalTempDir::with_prefix` | 使用自定义前缀在 `std::env::temp_dir()` 中创建临时目录。 |
| `LocalTempDir::in_dir` | 在调用方指定的父目录和重试次数下创建临时目录。 |

所有权方法：

| 方法 | 行为 |
| --- | --- |
| `path` | 借用生成的绝对目录路径。 |
| `exists` | 检查目录路径是否存在，返回 `std::io::Result<bool>`。 |
| `metadata` | 读取目录 metadata。 |
| `list` | 列出直接子项。 |
| `child_path` | 只对相对 child 路径做 lexical 校验，返回拼接后的绝对路径，不检查文件系统。 |
| `ensure_child_dir` | 创建 child 目录和缺失父目录，语义类似 `mkdir -p`，并返回绝对路径。 |
| `open_child_reader` | 使用 `FileReadOptions` 打开 child 文件 reader。 |
| `open_child_writer` | 使用 `FileWriteOptions` 打开 child 文件 writer。 |
| `cleanup` | 立即删除目录，并关闭后续 drop 清理。 |
| `keep` | 消费 guard，把目录留在生成位置，并返回绝对路径。 |
| `persist` | 把目录移动到最终路径，返回其绝对路径，并关闭自动清理。 |

`LocalTempDir::persist` 会为目标创建缺失父目录，并拒绝已存在目标。它不提供 overwrite 选项。如果移动失败，`LocalPersistError` 会把 guard 所有权返回给调用方，以便重试、保留、检查或显式清理；错误还会报告失败阶段、调用方传入的目标，以及解析成功后绑定的绝对目标。持久化只使用原生 move/rename，不会回退到 copy-and-delete，因此跨文件系统移动可能在 Unix 上返回 `EXDEV`，或返回其他平台的等价错误。

child 路径必须是非空相对路径，并且只能由 normal path component 组成。绝对路径、root 或 prefix component、`.` 和 `..` 都会被拒绝。`child_path` 在 lexical 校验后即返回；已有 symbolic-link component 仍可能解析到临时目录之外，因此返回路径不能证明文件系统 containment。`open_child_reader` 要求 child 是文件；目录或其他非文件条目会返回 `ErrorKind::InvalidInput`。`open_child_writer` 会校验已存在目标必须是文件，并确保 child 写入留在临时目录内。`ensure_child_dir` 会创建缺失的多层父目录，但在创建目录时会拒绝 symbolic link component，避免通过 child 路径离开临时目录。

这些检查假设不可信参与者不会在校验和使用之间替换路径 component。child helper 是便捷的 containment check，不是抵御并发文件系统修改的 capability-based sandbox 边界。

`Drop` 中的清理是 best-effort。如果删除失败，`LocalTempDir` 会通过 `log` 门面记录 warning，不会 panic。

## 临时文件

当你需要一个唯一临时文件路径和一个由 guard 持有的文件句柄时，使用 `LocalTempFile`。除非调用 `keep` 或 `persist`，否则文件会在 drop 时删除。在 Unix 上，临时文件以 `0600`、临时目录以 `0700` 创建，之后仍受进程 umask 约束。

```rust
use std::io::Write;

use qubit_local_files::LocalTempFile;

let mut file = LocalTempFile::with_affixes("qubit-local-files-", ".txt")?;
file.write_all(b"temporary payload\n")?;
file.close();

# Ok::<(), std::io::Error>(())
```

创建方法：

| 方法 | 用途 |
| --- | --- |
| `LocalTempFile::new` | 使用默认前缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::with_prefix` | 使用自定义前缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::with_suffix` | 使用默认前缀和自定义后缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::with_affixes` | 使用自定义前缀和后缀在 `std::env::temp_dir()` 中创建临时文件。 |
| `LocalTempFile::in_dir` | 在调用方指定的父目录和重试次数下创建临时文件。 |

句柄和所有权方法：

| 方法 | 行为 |
| --- | --- |
| `path` | 借用生成的绝对文件路径。 |
| `exists` | 检查文件路径是否存在，返回 `std::io::Result<bool>`。 |
| `metadata` | 读取文件 metadata。 |
| `as_file` / `as_file_mut` | 借用最初创建并持有的 `File` 句柄。 |
| `Write` / `Seek` | 直接通过持有的句柄写入或 seek。 |
| `close` | 丢弃无缓冲句柄但保留路径清理；它不调用 `sync_all`。 |
| `cleanup` | 立即删除文件，并关闭后续 drop 清理。 |
| `keep` | 关闭并消费 guard，把文件留在生成位置并返回绝对路径。 |
| `persist` | 不覆盖地把文件移动到最终路径，并返回绝对最终路径。 |
| `persist_with` | 使用 `LocalPersistOptions` 移动文件，并返回绝对最终路径。 |

`LocalTempFile` 有意不提供读取 helper。临时文件的常见用法是写入、关闭，然后持久化。如果确实需要检查内容，先调用 `close`，再通过 `LocalFiles::open_reader` 或 `std::fs` 读取 `path()`。

`LocalTempFile::persist` 会关闭文件，为目标创建缺失父目录，并通过 no-clobber move 操作拒绝已存在目标。它有意不依赖单独的 metadata precheck。这可以在支持的平台上避免 time-of-check/time-of-use 覆盖竞态。失败时返回 `LocalPersistError<LocalTempFile>`，保留 guard、原始 I/O error、`ResolveTarget` / `PrepareParent` / `InstallDestination` 阶段、调用方目标及可用的绝对目标。文件持久化只使用原生 move/rename，不会回退到 copy-and-delete，因此跨文件系统移动可能在 Unix 上返回 `EXDEV`，或返回其他平台的等价错误。启用 overwrite 时，最终文件保留临时文件的 metadata，而不是被替换目标的 metadata；如果替换内容时必须严格保留受支持的平台原生 metadata，请使用 `LocalFiles::atomic_write`。

只有覆盖策略确实不同的时候才使用 `persist_with`：

```rust
use std::io::Write;

use qubit_local_files::{LocalPersistOptions, LocalTempDir, LocalTempFile};

let dir = LocalTempDir::with_prefix("qubit-local-files-persist-")?;
let target = dir.path().join("result.txt");
std::fs::write(&target, "old")?;

let mut file = LocalTempFile::with_affixes("qubit-local-files-", ".txt")?;
file.write_all(b"new\n")?;

file.persist_with(&target, LocalPersistOptions::new().with_overwrite())?;

assert_eq!("new\n", std::fs::read_to_string(&target)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

如果目标文件不能被外部观察到“只写了一半”，最终文件替换优先使用 `LocalFiles::atomic_write`。

## No-Replace 平台支持

本 crate 使用原生 no-replace 安装原语，不使用 hard-link 或 copy-and-delete 模拟。支持矩阵如下：

| 操作 | Linux | macOS | Windows | 其他目标 |
| --- | --- | --- | --- | --- |
| 临时文件/目录默认持久化（不替换） | 支持 | 支持 | 支持 | `Unsupported` |
| 递归复制 `Fail`/`Skip` 文件提交 | 支持 | 支持 | 支持 | `Unsupported` |
| 临时文件 overwrite 持久化 | 支持 | 支持 | 支持 | 使用普通替换能力 |
| 递归复制 `Overwrite` | 支持 | 支持 | 支持 | 使用普通替换能力 |

在不支持的目标上，`LocalTempFile::persist`、禁用 overwrite 的 `LocalTempFile::persist_with` 和 `LocalTempDir::persist` 返回 `ErrorKind::Unsupported`，同时由 `LocalPersistError` 保留临时资源。递归复制的 `Fail` 或 `Skip` 会报告 `LocalCopyDirStage::CommitFile` 和 `ErrorKind::Unsupported`。此时可能已经创建了一部分目标目录；递归复制不提供整个事务的回滚。overwrite 操作使用普通替换原语，不受 no-replace 支持矩阵限制。

## 根目录 Capability

`LocalRoot` 打开目录 descriptor，并把该 descriptor 作为所有后代操作的 authority。保存的绝对 root 路径仅用于诊断。后代名称使用 `LocalRelativePath` 表示，reader、writer 和 atomic writer 的遍历会拒绝每一级 component 上的 symbolic link。root 路径或中间名称被重命名或替换时，已经打开的 descriptor 不会被重定向。

这里保证的是 descriptor-relative 路径 containment，不是 inode 名称唯一性或完整的 OS 安全边界。hard link、mounted filesystem、权限以及拥有同等 OS authority 的进程仍属于部署安全责任。该 backend 在 Unix 上可用；其他目标返回 `ErrorKind::Unsupported`，不会回退到 check-then-path。path-based `LocalFiles` 是便利 API；当其他参与者可以并发修改 namespace 时，不能作为 sandbox 边界。

## Atomic Write

`LocalFiles::atomic_write` 会在同一父目录下写入临时文件，flush 并 sync 这个临时文件，替换目标，并在支持的平台上从深到浅 sync 目标父目录以及本次新建目录项所在的各级父目录。目标必须不存在或是已有普通文件；symbolic link、目录、FIFO、socket、device 和其他特殊文件会以 `ErrorKind::InvalidInput` 拒绝。

```rust
use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix("qubit-local-files-guide-")?;
let path = dir.path().join("state").join("manifest.json");

LocalFiles::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;

assert_eq!(
    br#"{"version":1,"complete":true}"#,
    std::fs::read(&path)?.as_slice(),
);

# Ok::<(), Box<dyn std::error::Error>>(())
```

当内容生成逻辑应在受保护的原子写 callback 中执行时，使用
`LocalFiles::atomic_write_with`。callback 收到支持 `Write` 的
`LocalAtomicWriter`，但不能 clone 或保留底层文件句柄：

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix("qubit-local-files-json-")?;
let path = dir.path().join("state.json");

LocalFiles::atomic_write_with(&path, |writer| {
    writeln!(writer, "{{\"complete\":true}}")
})?;

assert_eq!("{\"complete\":true}\n", std::fs::read_to_string(&path)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

需要跨多次调用 streaming 内容时，使用 `LocalAtomicWriter`：

```rust
use std::io::Write;
use qubit_local_files::LocalFiles;

let mut writer = LocalFiles::begin_atomic_write("state.bin")?;
writer.write_all(b"complete state")?;
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`LocalAtomicWriter` 实现 `Write`，但首版不实现 `Seek`。只有 `commit`
成功后目标才会被替换；调用 `abort` 或直接 drop 会保留原目标并清理 staging
文件。自 `0.5.0` 起，配置类型的字段不再公开，调用方必须使用现有 getter、
constructor 和 builder。API 仍保持同步边界。

### 已有目标的 metadata 契约

metadata 保留采用严格语义。读取、ACL、xattr/extattr 或原生 merge 任一步骤
失败，都会中止替换，不会在降低保护后仍返回成功。

| 目标平台 | 从当前目标保留的 metadata |
| --- | --- |
| Windows | flags 为 `0` 的 `ReplaceFileW` 保留 creation time、short name、object identifier、DACL、security resource attributes、encryption、compression，以及 staging 中不存在的 named stream。 |
| Linux / Android | uid、gid、完整 mode 与所有 descriptor-visible xattr，包括平台暴露的 POSIX ACL、SELinux 和 capability attribute。 |
| macOS | 通过 descriptor 操作与 `fcopyfile` 保留 uid、gid、mode、ACL 和 xattr。 |
| FreeBSD | uid、gid、mode、受支持的 POSIX 或 NFSv4 ACL，以及 user/system extattr。 |

这些行描述已经实现的代码路径。Android 与 FreeBSD 是 compile-only 目标，因此
本仓库 CI 不对表中所列行为做 runtime 验证。

crate 不会为了强制替换而清除 `FILE_ATTRIBUTE_READONLY`；只读 Windows 目标会被
`ReplaceFileW` 拒绝并保持不变。

Unix metadata 在 `commit` 时从目标 handle 读取，而不是 writer 开始时的快照。
staging metadata 会在替换前同步，并再次校验目标 device/inode identity。Unix
有意不承诺保留 inode 或 hard-link identity、mtime/ctime、immutable 或
append-only flag；Windows 遵循 `ReplaceFileW` 的原生 merge 契约。

重要语义：

- 写入前会创建父目录。
- 临时文件创建在目标目录下，因此在常见本地文件系统上可以 atomic replacement。
- 已有目标的 Unix metadata 在 commit 时从当前打开的目标捕获；Windows 在
  `ReplaceFileW` 内合并 metadata。
- writer 开始时不存在的目标通过原生 no-replace 操作安装，绝不会覆盖并发
  创建者。
- 在 Unix 上，新目标使用 `0600`，之后仍受更严格的进程 umask 约束。
- 如果写入、flush 或 sync 临时文件失败，目标保持不变。
- 如果 `atomic_write_with` callback panic，unwind 会先关闭并 best-effort 删除未提交临时文件，再继续传播 panic；目标保持不变。清理失败不能替换原 panic，因此 staging path 可能残留。
- 如果替换已经成功，但 sync 目标父目录或新建目录项的父目录失败，方法可能在目标已经包含新内容后返回错误。
- 错误通过 `LocalAtomicWriteError` 报告，包含失败阶段、临时路径、原始 I/O source、destination state，以及 secondary staging cleanup error。`Unchanged`、`Replaced`、`Missing` 与 `Indeterminate` 对应不同恢复动作；只有 `Unchanged` 会自动清理 staging，其他结果保留仍存在的 staging entry。
- 最终目标检查和替换仍是两个 path-based 操作；需要抵御并发 namespace 替换时，应使用 `LocalRootAtomicWriter`。
- 该操作不是多文件事务，也不协调并发写入。

## 文件和目录 Helper

`LocalFiles` 提供小型本地文件系统 helper：

| 方法 | 行为 |
| --- | --- |
| `exists` | 检查路径是否存在，并且不吞掉检查错误。 |
| `metadata` | 通过 `std::fs::metadata` 读取路径 metadata。 |
| `list` | 列出目录直接子项。 |
| `open_reader` | 使用 `FileReadOptions` 把普通文件打开为 `LocalFileReader`；拒绝目录和特殊资源。 |
| `open_writer` | 使用 `FileWriteOptions` 打开或创建普通文件 `LocalFileWriter`；拒绝目录和特殊资源。 |
| `ensure_dir` | 创建目录及缺失祖先目录。 |
| `ensure_parent` | 为文件路径创建缺失父目录。没有父目录的路径会被接受。 |
| `dir_size` | 统计目录下普通文件总字节数，不跟随 symbolic link。 |
| `clean_dir` | 删除所有子项但保留目录自身。 |
| `remove_any` | 删除文件、目录树或 symbolic link。 |

示例：

```rust
use std::io::Write;

use qubit_local_files::{
    FileReadOptions,
    FileWriteMode,
    FileWriteOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix("qubit-local-files-helpers-")?;
let path = dir.path().join("nested").join("data.txt");

let mut writer = LocalFiles::open_writer(
    &path,
    FileWriteOptions::new(FileWriteMode::CreateOrTruncate)
        .with_parent()
        .buffered(),
)?;
writer.write_all(b"payload")?;
writer.close()?;

let mut reader = LocalFiles::open_reader(&path, FileReadOptions::buffered())?;
let mut payload = String::new();
std::io::Read::read_to_string(&mut reader, &mut payload)?;
assert_eq!("payload", payload);

assert_eq!(7, LocalFiles::dir_size(dir.path())?);
LocalFiles::clean_dir(dir.path())?;
assert_eq!(0, LocalFiles::dir_size(dir.path())?);

# Ok::<(), std::io::Error>(())
```

`dir_size` 和 `clean_dir` 要求根路径是目录。symbolic link 不会被跟随。`remove_any` 会删除 link 本身，包括指向目录的 link。

## 递归目录复制

当目录树复制需要显式冲突策略和 symlink 策略时，使用 `LocalFiles::copy_dir_all_with`。

```rust
use qubit_local_files::{
    LocalCopyDirOptions,
    LocalFiles,
    LocalTempDir,
};

let dir = LocalTempDir::with_prefix("qubit-local-files-copy-")?;
let src = dir.path().join("src");
let dst = dir.path().join("dst");

LocalFiles::ensure_dir(&src)?;
std::fs::write(src.join("data.txt"), b"data")?;

let stats = LocalFiles::copy_dir_all_with(&src, &dst, LocalCopyDirOptions::default())?;

assert_eq!(1, stats.files);
assert_eq!(1, stats.directories);
assert_eq!(4, stats.bytes);

# Ok::<(), Box<dyn std::error::Error>>(())
```

选项：

| 选项 | 默认值 | 行为 |
| --- | --- | --- |
| `with_conflict(...)` | `Fail` | 已存在目标文件会被拒绝；可显式选择 `Overwrite` 或 `Skip`。 |
| `with_type_conflict(...)` | `Fail` | 文件/目录类型冲突会被拒绝；`Replace` 显式允许破坏性替换。 |
| `follow_symlinks()` | `false` | 源目录树中的 symbolic link 会被拒绝。 |
| `preserve_permissions()` | `false` | 不复制源权限；在 Unix 上，新建或替换的文件保留 `0600`，新建目录使用 `0700`，之后仍受进程 umask 约束。 |

`Fail` 和 `Skip` 文件提交需要原生 no-replace 原语，因此在 Linux、macOS、Windows 之外返回 `ErrorKind::Unsupported`。`Overwrite` 使用普通替换原语。由于操作不是目录树级事务，在不支持的文件提交前已经创建的目标目录会继续保留。

统计信息：

| 字段 | 含义 |
| --- | --- |
| `files` | 已复制的普通文件数量。 |
| `directories` | 已创建的目标目录数量。 |
| `bytes` | 从普通文件复制的字节数。 |
| `skipped` | 因冲突策略而跳过的已有目标文件数量。 |

复制操作会拒绝位于源目录内部的目标，因为把目录复制进自身可能导致无限递归。当启用 symlink following 时，由跟随 symbolic link 引入的目录环也会被拒绝。打开后确认不是普通文件的源条目通过 `LocalCopyDirError` 报告 `std::io::ErrorKind::InvalidInput`；显式跟随 symbolic link 后遇到不支持的目标类型则报告 `ErrorKind::Unsupported`。结构化错误同时提供失败阶段、源和目标路径、部分统计、可选 staging path、可选次级 cleanup error 及原始 I/O source error；原始复制或提交错误保持为主 source error。递归复制不是目录树级事务：失败前已经提交的条目会留在目标中，不会执行回滚；破坏性的类型冲突替换还可能先删除已有目标目录，随后才在后续操作中失败。

普通文件类型和可选文件权限来自实际提供复制字节的同一个已打开 handle。禁用 link 时 Unix 使用 `O_NOFOLLOW`，Windows 拒绝 name-surrogate reparse handle。目录遍历、目标复查和破坏性替换仍是 path-based 操作；当其他参与者能够并发修改任一目录树时，该策略不是可抵御攻击者的 sandbox 边界。

## 文件名 Helper

`LocalFilenames` 提供不访问文件系统的 lexical helper。返回文件名数据的方法返回 UTF-8 字符串（`&str` 或 `String`），而不是 `OsStr`；无效 UTF-8 path component 返回 `None`。

```rust
use std::path::Path;

use qubit_local_files::LocalFilenames;

let path = Path::new("/tmp/archive.tar.gz");

assert_eq!(Some("archive.tar"), LocalFilenames::file_stem(path));
assert_eq!(Some("archive"), LocalFilenames::file_prefix(path));
assert_eq!(Some("gz"), LocalFilenames::extension(path));
assert_eq!(Some(".gz".to_owned()), LocalFilenames::dot_extension(path));
assert!(LocalFilenames::has_extension(path, ".gz"));
assert!(LocalFilenames::has_extension_ignore_ascii_case(path, "GZ"));

let name = LocalFilenames::try_random_with(Some("upload-"), Some(".tmp"))?;
assert!(name.starts_with("upload-"));
assert!(name.ends_with(".tmp"));

# Ok::<(), std::io::Error>(())
```

当调用方提供的名称应该是跨常见平台保守安全的单 path component 时，使用 `validate_portable_file_name`：

```rust
use std::io::ErrorKind;

use qubit_local_files::LocalFilenames;

LocalFilenames::validate_portable_file_name("report.csv")?;

let error = LocalFilenames::validate_portable_file_name("CON.txt")
    .expect_err("Windows reserved names are rejected");
assert_eq!(ErrorKind::InvalidInput, error.kind());

# Ok::<(), std::io::Error>(())
```

portable 校验是 lexical 的。它不检查当前文件系统权限、mount option、Unicode normalization，或每个文件系统独有的限制。
它也会拒绝 `COM¹`、`COM²`、`COM³`、`LPT¹`、`LPT²` 和 `LPT³`：Windows 会把 ISO/IEC 8859-1 上标数字视为 device-name digit，详见 [Microsoft 文件命名规则](https://learn.microsoft.com/zh-cn/windows/win32/fileio/naming-a-file)。

对于还不是 `Path` 的字符串，可以使用字符串 helper：

```rust
use qubit_local_files::LocalFilenames;

assert_eq!("file.txt", LocalFilenames::file_name_from_path(r"C:\tmp\file.txt"));
assert_eq!(
    "report 2026.csv",
    LocalFilenames::file_name_from_url("https://example.test/files/report%202026.csv?download=1"),
);
```

`file_name_from_url` 会先去掉 query 和 fragment，再选择最后一个 slash-delimited segment。只有当 percent-encoded UTF-8 解码后仍然是安全的单文件名 fragment 时，它才返回解码结果。

## 错误和清理模型

简单 API 返回 `std::io::Result` 并保留原始 error chain。atomic write、递归复制和临时资源持久化使用结构化错误，携带安全恢复需要的额外状态。

重要错误行为：

- `LocalPersistError` 保留临时资源、失败阶段、调用方目标、可用的绝对目标和原始错误。
- `LocalAtomicWriteError::destination_state()` 返回 `Unchanged`、`Replaced`、`Missing` 或 `Indeterminate`；最后一种结果要求恢复前检查目标和 staging 两条路径。
- 临时文件持久化目标已存在时会被拒绝，除非显式设置 `LocalPersistOptions::new().with_overwrite()`。
- 临时目录持久化目标已存在时会被拒绝。
- 递归复制通过 `LocalCopyConflictPolicy` 处理已有文件，通过独立的 `LocalCopyTypeConflictPolicy` 处理文件/目录类型冲突。
- 递归复制遇到 symbolic link 时会被拒绝，除非显式设置 `LocalCopyDirOptions::new().follow_symlinks()`。
- Drop 阶段清理失败会通过 `log::warn!` 记录，不会 panic。
- `LocalTempFile::as_file`、`as_file_mut`、`Write` 和 `Seek` 在 `close` 之后返回 `ErrorKind::NotFound`。
- `LocalTempDir` child API 会在不安全 child 路径、child reader 目标不是文件、以及通过 symbolic link 离开临时目录时返回 `ErrorKind::InvalidInput`。

## 路径长度和平台限制

`LocalTempFile` 和 `LocalTempDir` 创建的是本地文件系统条目；如果创建失败，会返回操作系统错误。它们不承诺生成的路径适用于所有平台 API。某些 API，例如 Unix domain socket，有比普通文件短得多的路径限制。遇到这类场景，应在较短的父目录下创建临时条目，例如 `/tmp`。

临时资源和 atomic writer 使用的相对输入会在资源创建或操作开始时绑定到当时的进程工作目录。临时资源的 `path`、child path、`keep` 和持久化方法返回绝对路径，之后即使工作目录变化也可直接使用。在 Windows 上，crate 会拒绝内部 UTF-16 NUL，但不会添加 verbatim-path prefix，因此路径长度和 verbatim path 语义仍遵循原生平台行为。

## Crate 边界

`qubit-local-files` 有意把本地文件系统工具从 `qubit-io` 中分离出来。需要本地路径、临时文件和目录、递归目录操作、目录清理、文件名 helper 和 atomic file write 时，使用本 crate。

需要 stream trait、extension method、stream wrapper、内容比较、有界读取或 binary codec 时，使用
[qubit-io](https://github.com/qubit-ltd/rs-io)。

## 测试和 CI

本项目包含公开 helper、临时条目、覆盖语义、递归复制行为、文件名校验、atomic write 和平台相关边界情况的测试。

支持层级正式定义如下：

| 支持层级 | 目标平台 | CI 验证方式 |
| --- | --- | --- |
| 原生 runtime-tested | Linux、Windows、macOS | 测试在对应操作系统上执行，并覆盖平台相关文件系统行为。 |
| Compile-only | FreeBSD、Android | CI 使用 `cargo check` cross-compile production 与 cfg-selected source；本仓库不验证或保证其 runtime 文件系统、ABI 与 metadata 行为。 |

常用命令：

```bash
cargo test
./coverage.sh
./align-ci.sh
./ci-check.sh
```
