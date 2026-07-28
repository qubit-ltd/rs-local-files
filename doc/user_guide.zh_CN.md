# Qubit Local Files 用户手册

Qubit Local Files 是应用和本地 provider adapter 使用的 concrete native 文件系统层。
它不定义 provider path、registry 或远程文件系统行为。

## API 模型

公共 API 提供两种 authority：

- `LocalFileSystem` 通过关联方法组织 host-wide 操作；
- `RootedLocalFileSystem` 是绑定已打开目录 descriptor/handle 的有状态 authority。

`LocalFileNames` 与 `LocalPaths` 提供 native lexical helper。Reader、writer、walker
和临时资源都是有状态值。文件系统操作不提供 public free-function alias。

## Host 操作

```rust
use std::io::{Read, Write};

use qubit_local_files::{
    LocalCreateDirectoryOptions,
    LocalFileSystem,
    LocalReadOptions,
    LocalWriteMode,
    LocalWriteOptions,
    LocalWriterState,
};

let root = std::path::Path::new("build/output");
LocalFileSystem::create_directory(
    root,
    &LocalCreateDirectoryOptions::new().with_recursive(),
)?;

let path = root.join("manifest.json");
let mut writer =
    LocalFileSystem::open_writer(
        &path,
        &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    )?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(LocalWriterState::Committed, result.state());

let mut reader =
    LocalFileSystem::open_reader(&path, &LocalReadOptions::new())?;
let mut text = String::new();
reader.read_to_string(&mut text)?;

# Ok::<(), Box<dyn std::error::Error>>(())
```

跨多个系统调用的相对路径会在操作开始时绑定。Copy 和 rename 使用同一个 current
directory snapshot 绑定 source 与 target。Metadata 默认观察 final entry 本身，不跟随
final symbolic link。

## Copy

`LocalFileSystem::copy` 与 `RootedLocalFileSystem::copy` 根据 source metadata 选择文件
或目录行为。

```rust
use qubit_local_files::{LocalCopyOptions, LocalFileSystem};

let outcome = LocalFileSystem::copy(
    std::path::Path::new("source"),
    std::path::Path::new("backup"),
    &LocalCopyOptions::new(),
)?;
assert!(outcome.stats().files() + outcome.stats().directories() > 0);

# Ok::<(), Box<dyn std::error::Error>>(())
```

Option 分别描述 target conflict、type conflict、metadata、symbolic-link、
device-boundary、递归、atomicity 和 durability 策略。无法满足 required guarantee 时，
操作会在 destructive change 前拒绝执行。Copy 拒绝 self-copy 和 hard-link alias；
overwrite 会替换 target symbolic-link entry，而不是跟随它。

## 惰性遍历

```rust
use qubit_local_files::{LocalFileSystem, LocalListOptions};

let walker = LocalFileSystem::list(
    std::path::Path::new("workspace"),
    &LocalListOptions::new().with_max_depth(2),
)?;
for entry in walker {
    let entry = entry?;
    println!("{}", entry.path().display());
}

# Ok::<(), Box<dyn std::error::Error>>(())
```

Walker 按需打开并推进目录。Depth 与 symbolic-link policy 在创建时固定。Drop walker
只释放 handle，不修改 namespace。

## Writer 生命周期

`LocalWriteMode::CreateNew` 和 `CreateOrReplace` 使用同目录 staging；`Append` 直接修改
已有普通文件，因此拒绝 required atomicity。

Writer 初始状态为 `Open`。`commit` 返回 `LocalWriteOutcome`，`abort` 丢弃未发布的
staging。Stream write 或 flush 失败会进入 indeterminate 状态，因为普通 I/O error
不能证明没有字节发生变化。Commit failure 使用 `LocalFileCommitError` 保留
publication state。

## 临时资源

```rust
use std::io::Write;

use qubit_local_files::{
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};

let directory = LocalFileSystem::create_temp_directory(
    &LocalTempDirectoryOptions::new().with_suffix(".work"),
)?;
let mut file = LocalFileSystem::create_temp_file(
    &LocalTempFileOptions::new()
        .with_parent(directory.path())
        .with_suffix(".data"),
)?;
file.write_all(b"payload")?;
file.close();

# Ok::<(), Box<dyn std::error::Error>>(())
```

临时条目拥有 cleanup responsibility。只要 ownership 仍然 armed，Drop 就执行
best-effort cleanup。`keep` 会禁用 cleanup 并返回稳定绝对路径。Persist failure
保留资源，调用者可以 retry、inspect、keep 或显式 cleanup。

Prefix 与 suffix 在创建条目前校验。Native separator、NUL 和 portable reserved-name
违规不会留下临时条目。

## Rooted Authority

```rust
use qubit_local_files::{
    LocalListOptions,
    RootedLocalFileSystem,
};

let root =
    RootedLocalFileSystem::open(std::path::Path::new("workspace"))?;
let walker = root.list(
    std::path::Path::new("assets"),
    &LocalListOptions::new(),
)?;
for entry in walker {
    println!("{}", entry?.path().display());
}

# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted path 必须是 relative descendant。Absolute path、prefix、`.` 与 `..` 会被拒绝，
中间 symbolic link 不会被接受。诊断 root path 不参与授权：open 后 rename 该路径不会
重定向已经打开的 authority。

## 文件名与路径

Filename accessor 返回 `OsStr` 或 `OsString`，保留 Unix 非 UTF-8 名称和 Windows
native value。

```rust
use qubit_local_files::{LocalFileNames, LocalPaths};

let name = LocalFileNames::random_name_with(
    Some("upload-"),
    Some(".tmp"),
)?;
LocalFileNames::validate_portable(name.as_os_str())?;

let child = LocalPaths::compose_descendant(
    std::path::Path::new("workspace"),
    std::path::Path::new(name.as_os_str()),
)?;
assert!(LocalPaths::is_lexically_within(
    &child,
    std::path::Path::new("workspace"),
)?);

# Ok::<(), Box<dyn std::error::Error>>(())
```

相关路径应使用 `bind_host_paths`，由一个 current-directory snapshot 定义操作。
Lexical containment 只是 early classification，不能替代 descriptor-relative
authorization。

## 错误与 Capability

`LocalFileError` 提供 `LocalFileErrorKind`、`LocalFileOperation`、primary/target native
path，以及可选 `std::io::Error` source。Publication session 使用专用 failure type
表达 partial-success state。

`LocalFileSystem::capabilities()` 报告 host 实现；
`RootedLocalFileSystem::capabilities()` 返回 open authority 时缓存的 snapshot。
Path limit 显式携带单位：Unix 为 byte，Windows 为 UTF-16 code unit。

## 验证

```bash
cargo test --all-features
./align-ci.sh
./ci-check.sh
```

Linux、Windows 和 macOS 执行 runtime test；FreeBSD 与 Android 只 compile-check，
不会被描述为 runtime 保证。
