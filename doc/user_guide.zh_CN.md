# Qubit Local Files 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) ·
[设计文档](local_file_system_design.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-local-files)

## 手册目标与读者

本手册面向 Rust 1.94 及以上版本的 `qubit-local-files` 0.3 使用者，适用于直接操作主机
文件系统，或需要把操作限制在一个已打开目录之下的应用。它不是 provider 注册表、远程
文件系统 API，也不替代 provider 层的逻辑路径模型。本 crate 提供同步 API；异步应用应在
合适的 blocking 执行环境中调用。

## 操作契约

`LocalFileSystem` 会在解析路径或修改文件系统前校验静态选项组合。复制失败会报告已证明的
最强目标状态：`Unchanged` 表示已证明没有修改目标，`Indeterminate` 表示无法确定原生修改
的结果。即使打开读取器失败，`read_prefix` 错误仍保留外层的 `Read` 操作。

## 概念模型

```text
Host 命名空间 ── LocalFileSystem::host() ── 操作时读取进程 PWD
已打开根目录 ─── LocalFileSystem::rooted(root) ── 虚拟 / 与实例 PWD
```

`LocalFileSystem` 是有状态的文件系统对象。`host()` 选择进程可见命名空间，但构造时
不读取当前目录；Host 绝对路径从不依赖 PWD，相对路径在操作开始时捕获一次进程 PWD。
`rooted(root)` 打开唯一目录 authority，将其映射为虚拟
根 `/`，初始 PWD 为 `/`。两种形式都接受 namespace-absolute 路径以及相对于实例
对应 PWD 的路径，并提供相同操作。reader、writer、walker 与临时条目都是拥有资源的有状态
对象。`LocalFileNames` 和 `LocalPaths` 提供原生词法工具，不会把文件名强制转换为
UTF-8。

## 安装与最小配置

在应用的 Cargo 清单中添加依赖：

```toml
[dependencies]
qubit-local-files = "0.3"
```

配置操作策略前，先选择权限范围。Host 模式使用进程可见的命名空间；Rooted 模式打开一个
已存在目录并将其映射为虚拟 `/`。权限句柄打开后，构造时传入的 Host 路径只用于诊断。

```rust,no_run
use std::path::Path;

use qubit_local_files::LocalFileSystem;

let host = LocalFileSystem::host()?;
let rooted = LocalFileSystem::rooted(Path::new("workspace"))?;
assert!(host.diagnostic_root().is_none());
assert!(rooted.diagnostic_root().is_some());
# Ok::<(), Box<dyn std::error::Error>>(())
```

## 配置一次，显式覆盖

每个 Rooted filesystem 实例拥有自己的虚拟 PWD；Host 实例观察进程全局 PWD。每个实例
都拥有符号链接策略，以及 read、write、list、copy、
create-directory、delete、rename、temporary-file 和 temporary-directory 九种默认
Options。调用方可以通过 `set_default_*_options` 一次配置，然后使用普通操作方法。

每个 `*_with_options` 方法则把传入的 Options 当作该次调用的完整配置，不会与实例
默认值合并。需要在默认值基础上只修改一个字段时，应显式 clone 或 copy 对应默认值，
修改后再传入。

```rust,no_run
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalListOptions;

let mut filesystem = LocalFileSystem::rooted(std::path::Path::new("/srv/app"))?;
filesystem.set_current_directory(std::path::Path::new("/assets"))?;
filesystem.set_default_list_options(
    LocalListOptions::new().with_recursive().with_max_entries(10_000),
)?;

let default_walk = filesystem.list(std::path::Path::new("."))?;
let one_level = filesystem.list_with_options(
    std::path::Path::new("."),
    &LocalListOptions::new(),
)?;
# drop((default_walk, one_level));
# Ok::<(), Box<dyn std::error::Error>>(())
```

初始 Options 不包含隐藏的业务资源上限。遍历和复制预算、重试时长、deadline 与临时名称
尝试次数，只有调用方显式设置后才生效。clone filesystem 会复制 Rooted 虚拟 PWD 与全部配置；
Host clone 继续观察同一进程 PWD；
Rooted clone 只共享不可变的已打开 authority。本 crate 不承诺共享可变配置时的同步；
调用方应每线程持有一个 clone，或自行添加同步包装。

## 符号链接策略

`LocalFileSystem` 实例保存一个由所有操作继承的符号链接策略。
`LocalFileSystem::rooted(root)` 默认使用 `FollowWithinScope`：允许跟随链接，
但解析结果必须仍位于已打开的 root 内。Host 默认使用 `FollowAcrossScope`，因为
Host 没有更窄的 root 边界。Rooted 仅支持 `Reject` 和 `FollowWithinScope`；配置
`FollowAcrossScope` 会返回 `InvalidOptions`。可失败的 `set_symlink_policy` 以及
`list`、`copy` options 可以选择受支持的策略。

策略作用于所有中间路径组件。Rooted 使用 `FollowWithinScope` 时，像
`etc/link/config` 这样的路径若通过 `link` 越出已打开 root，会返回 `InvalidPath`。
`FollowAcrossScope` 仅适用于 Host。Rooted 中以 `/` 开始的链接目标会从虚拟根重新
开始，而不是从 Host 根开始。`.` 与 `..` 在链接目标中保留原生词法语义，但 `..`
一旦越过虚拟根就返回 `InvalidPath`。

最终路径组件遵循真实文件系统中的操作语义：

| 操作 | 最终符号链接 |
| --- | --- |
| `metadata` | 查看链接条目本身。 |
| `open_reader` | 有效策略允许时跟随内容目标；使用 `Reject` 时返回错误。 |
| `CreateNew` writer | 将已有链接视为已存在条目。 |
| `Append` writer | 跟随链接追加到目标。 |
| `CreateOrReplace` writer | 跟随链接替换目标，并保留链接。 |
| `delete_file` | 删除链接条目本身，包括指向目录的链接。 |
| `delete_directory` | 最终条目是链接时返回 `NotDirectory`。 |
| `rename` | 移动或替换链接条目。 |
| `copy` 源 | 复制链接条目本身。 |
| `copy` 目标 | 替换目标链接条目。 |
| `temp persist` | 通过 rename 发布并替换目标链接条目。 |

Reader 和 writer 最终只接受普通文件内容。即使策略允许跟随链接，其目标也必须是普通文件；
目录和特殊文件都会被拒绝。

目录遍历在有效策略允许时跟随目录链接。返回路径保持逻辑路径，例如 `link/child`，而不
是规范化后的目标路径；递归遍历按底层目录对象身份检测循环。深度限制按逻辑路径条目计算，
穿过链接不会额外增加一层。

## 场景：写入并检查导出产物

导出程序需要创建 `build/output`，只在完整写入后发布 `manifest.json`，并读回结果。成功
的可观察条件是 writer 返回 `Committed`，且能读取已经发布的字节。

```rust,no_run
use std::io::{Read, Write};
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{
    LocalCreateDirectoryOptions, LocalWriteMode, LocalWriteOptions,
};
use qubit_local_files::outcome::LocalWriterState;

let mut filesystem = LocalFileSystem::host()?;
filesystem.set_default_create_directory_options(
    LocalCreateDirectoryOptions::new().with_recursive(),
)?;
filesystem.set_default_write_options(LocalWriteOptions::new(
    LocalWriteMode::CreateOrReplace,
))?;

let output = std::path::Path::new("build/output");
filesystem.create_directory(output)?;
let path = output.join("manifest.json");
let mut writer = filesystem.open_writer(&path)?;
writer.write_all(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut text = String::new();
filesystem.open_reader(&path)?
    .read_to_string(&mut text)?;
assert_eq!(text, r#"{"complete":true}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

多次系统调用使用的相对路径会在操作开始时绑定。复制和重命名会用同一个当前目录快照绑定
源和目标。`metadata` 观察最终条目本身，不跟随最终符号链接。

## 发布、复制与恢复

`CreateNew` 与 `CreateOrReplace` 使用目标目录内的暂存；`Append` 直接修改已有普通文件，
因此拒绝要求的原子性。writer 的生命周期（`LocalWriterState`）与发布结论
（`LocalWriteFailureState`）分离。`Interrupted` 和 `WouldBlock` 允许重试；其他流错误会
禁止继续写入和提交：暂存写入保留 `NotPublished`，追加写入在此前已成功写入字节时为
`Published`，否则为 `NotPublished`。仍可调用 abort 清理。提交失败仍可能报告
`Indeterminate`，需要恢复时应保留并检查返回的资源或错误。向量写入可能成功但只写入
部分字节，调用方应按返回的字节数推进缓冲区。当同时启用 `create_parent` 与 Required 耐久性时，
atomic writer 会创建缺失的祖先目录，并在发布后逐一同步新建目录；该阶段失败会以 `Published` 和
不完整发布错误报告。

`LocalFileSystem::copy` 根据源元数据选择文件或目录行为。需要固定源类型时使用
`with_entry_source()` 或 `with_tree_source()`，并通过 `source_mode()` 读取模式。
Copy Options 分别控制目标冲突、类型冲突、元数据、符号链接、原子性、耐久性以及由调用方
选择的资源预算；复制策略不包含 mount 或 device 边界。无法满足的要求保证会在破坏性变更
前被拒绝。自复制和硬链接别名会被拒绝；覆盖符号链接目标时会替换该条目而不跟随它。

```rust,no_run
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::outcome::LocalCopyFailureState;

let filesystem = LocalFileSystem::host()?;
match filesystem.copy_with_options(
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
# Ok::<(), Box<dyn std::error::Error>>(())
```

重命名也会通过类型化失败状态报告 `Unchanged`、`Renamed` 或 `Indeterminate`；出错并不等于
“什么都没发生”。

### 复制源模式

Host 与 Rooted 中，`files()` 包括复制的链接，`directories()` 只计入新建目录，
`bytes()` 只计入普通文件字节。`overwritten()` 包括替换的条目和 `Overwrite` 策略下
合并的已有目录，包括复制根目录；`Skip` 策略下的目录合并不计为覆盖。

| 模式 | 普通文件 | 最终链接，包括悬空链接 | 实体目录 | 特殊文件 |
| --- | --- | --- | --- | --- |
| `Entry` | 复制内容 | 复制链接本身 | `RequirementNotMet` | `Unsupported` |
| `Tree` | `RequirementNotMet` | `RequirementNotMet` | 复制目录树 | `Unsupported` |
| `Auto` | 按条目复制 | 按条目复制 | 按目录树复制 | `Unsupported` |

`with_entry_source()` 选择单个普通文件或链接条目，`with_tree_source()` 要求源为实体目录。
`with_source_mode(LocalCopySourceMode::Auto)` 会明确恢复自动判断。源类型拒绝发生在创建
目标父目录或修改目标之前。目录限定路径语法单独校验，可能在分派前返回 `NotDirectory`。
旧的 `File` 变体和 `with_file_source()` 方法已移除，不保留兼容别名。
源模式判断不跟随最终链接；递归目录树内部遇到的目录链接仍按有效遍历策略处理。
源模式不会改变中间链接解析或目录树的链接遍历语义。

目录树复制无法提供强制原子性或持久性，链接复制无法提供强制原子性。请求无法满足的
`Required` 保证时，会在修改目标前返回 `RequirementNotMet`。链接持久性取决于平台能力，
以及目标父目录和新建祖先目录的同步结果。
Windows 复制链接会保留源链接的文件/目录类型，包括悬空链接；覆盖目标链接时不会删除
该链接指向的内容。

## 遍历和临时资源

`LocalFileSystem::list` 返回惰性的 `LocalDirectoryWalker`。它按需打开和推进目录；
规范化 root、Options、符号链接策略、PWD snapshot 和 authority 在创建时固定。默认不设置
深度、条目数、名称内存、deadline 或打开目录数预算。调用方设置打开目录预算后，
`Reopen` 会按需关闭并重新打开活动 frame，`Fail` 则会在边界返回 `ResourceLimit`。
零句柄预算无效并返回 `InvalidOptions`。Rooted 会逐项读取目录，避免先收集到 `Vec`；
drop walker 只释放句柄。Host 列举即使跟随符号链接进入另一个物理目录，也会保留请求的
namespace 路径作为公开 root；可选的 diagnostic path 仍可记录实际访问的物理路径。

临时文件和目录在仍处于 armed 状态时拥有清理责任。每个资源都创建在独立的私有 sandbox 中，
sandbox 会和资源一起清理。需要观察清理失败时应显式调用 `cleanup()`；drop 只会静默地尽力清理。
`keep` 会原子发布到 sandbox 外生成的 sibling 路径，返回 `LocalPersistOutcome`，其 cleanup state
会报告 sandbox 残留。未显式指定 parent 时，在该次操作捕获的 filesystem PWD 下创建。
`path()`、`keep` 和持久化结果对 Host 与 Rooted 都返回 namespace-absolute 路径，因此
后续 PWD 即使变化，也能把它们再次传给同一个 filesystem。持久化失败会保留资源，调用方
可重试、检查、保留或显式清理。创建前会校验前缀和后缀：原生分隔符、NUL 与便携保留名称
不会留下条目。除非调用方设置 `max_attempts`，名称冲突尝试次数没有上限。

## 场景：关闭临时文件后交给路径使用者

MIME 检测器和外部程序通常需要重新按路径打开文件。应先写完内容，关闭原生句柄，
再调用路径使用者，最后显式清理。`close()` 不会解除清理责任。下面用文件读取代替
外部程序，让示例无需额外安装工具就能运行；应用可在回调中改为启动自己的工具。

错误对同时保留主 I/O 错误与清理错误。创建失败发生在检查之前，保存在结构化错误
位置。正式应用可用具名错误类型包装这两个独立信息，避免清理错误覆盖主错误。

```rust
use std::io;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileError;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempFileOptions;

fn inspect_staged<R>(
    payload: &[u8],
    inspect: impl FnOnce(&Path) -> io::Result<R>,
) -> Result<R, (Option<io::Error>, Option<LocalFileError>)> {
    let filesystem = LocalFileSystem::host().map_err(|error| (None, Some(error)))?;
    let options = LocalTempFileOptions::new()
        .with_parent(&std::env::temp_dir())
        .with_max_attempts(16);
    let mut file = filesystem.create_temp_file_with_options(&options)
        .map_err(|error| (None, Some(error)))?;
    let staged = file.write_all(payload);
    file.close();
    let primary = staged.and_then(|()| inspect(file.path()));
    match (primary, file.cleanup()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err((Some(primary), None)),
        (Ok(_), Err(cleanup)) => Err((None, Some(cleanup))),
        (Err(primary), Err(cleanup)) => Err((Some(primary), Some(cleanup))),
    }
}

let bytes = inspect_staged(b"payload", |path| std::fs::read(path))
    .expect("staging, inspection, and cleanup should succeed");
assert_eq!(b"payload", bytes.as_slice());
```

## Rooted 工作区

处理工作区下不受信任的相对名称时，应使用 rooted 访问。

```rust,no_run
use qubit_local_files::LocalFileSystem;

let mut root = LocalFileSystem::rooted(std::path::Path::new("workspace"))?;
root.set_current_directory(std::path::Path::new("/assets"))?;
let walker = root.list(std::path::Path::new("."))?;
for entry in walker {
    println!("{}", entry?.path().display());
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

Rooted 相当于以已打开目录为根的私有命名空间：`/etc/hosts` 映射到该 authority 下，
`etc/hosts` 则从实例 PWD 开始。`.` 和空路径表示 PWD；`a/./b` 规范化为 `a/b`；
`a/../b` 规范化为 `b`。只要未越过虚拟 `/`，parent component 就是合法的；因此在
PWD `/` 下，`..` 和 `a/./.././../b` 都返回 `InvalidPath`。Rooted 始终拒绝
native prefix。

中间符号链接遵循实例策略。`FollowWithinScope` 会拒绝解析到 root 外的链接；Rooted
不支持 `FollowAcrossScope`，配置该策略会返回 `InvalidOptions`。构造时路径可通过
`diagnostic_root()` 取得，但它不是 authority；打开后重命名该路径不会重定向基于 handle
的操作。词法包含关系可用于早期分类，但不能替代基于 handle 的授权。
Windows Rooted 的符号链接读取、类型判断和创建均相对于已打开 handle 执行；复制链接自身时
不会打开其悬空或位于 authority 外的目标。

## 递归删除预算

`LocalDeleteOptions` 支持 `with_max_depth`、`with_max_entries`、
`with_max_pending_path_bytes` 和 `with_deadline`。默认均不限，配套 `without_*`
方法可独立移除限制。预算适用于递归目录删除；请求目录本身计为一个条目、深度为零。
发现子条目后，在加入工作队列前扣减条目预算；待处理路径按原生编码长度计费，出队即释放。
队列路径预算不包括分配器开销与枚举中的临时对象。Host 和 Rooted 均惰性枚举，
同一时间最多打开一个目录读取器。期限在原生操作之间检查，不能中断正在阻塞的 I/O。

删除任何条目前超限，返回 `ResourceLimit` 并保留类型化资源信息；已经删除条目后失败，
返回 `PublicationIncomplete`，仍保留资源信息。期限错误保留 `TimedOut` I/O 类别。
重试前应同时检查副作用分类与失败原因。

兼容查询将这两个维度分开：`LocalFileError::cause_kind()` 返回目前可知的底层原因，
`LocalFileError::effect_state()` 对 `PublicationIncomplete` 返回
`Some(LocalFileEffectState::PartiallyApplied)`，对 `Indeterminate` 返回
`Some(LocalFileEffectState::Indeterminate)`。普通错误无法推断副作用时返回 `None`；
这不表示 `Unchanged`。copy、rename、writer 和 persist 的专用 failure 类型仍是精确恢复
状态的权威来源。

实例默认 Options 是便利配置，不是强制上限；显式 `*_with_options` 会完整替换它们。
需要请求无法放宽的 provider 上限时，应配置 `qubit-fs-local::LocalResourcePolicy`。

删除操作有明确的类型契约：`delete_file` 遇到实体目录返回 `IsDirectory`，
`delete_directory` 遇到普通文件或最终符号链接返回 `NotDirectory`。`missing_ok` 只对请求的
根条目不存在时生效。递归删除已移除条目后失败时，`LocalFileError` 会保留
`PublicationIncomplete`；重试前应分别检查 `effect_state()` 与 `cause_kind()`。基础错误的
`effect_state()` 返回 `None` 表示没有足够证据推断副作用，不能当作 `Unchanged`。


## 错误与诊断

`LocalFileError` 包含 `LocalFileErrorKind`、`LocalFileOperation`、可用时的
namespace-absolute 主/目标路径、操作使用的 PWD snapshot，以及可选的 typed source。
物理路径只作为可选诊断信息，绝不定义 Rooted authority。发布操作用专门失败类型保存部分
成功状态。

`LocalPersistError` 同时保留临时资源和结构化的 `LocalFileError`；其 `state()` 是唯一的
恢复状态来源。存在原生 I/O 错误时，可从结构化错误的 source 取得。

基础错误提供了增量兼容查询，调用方可以在不取得错误所有权的情况下分别读取两个维度：

```rust,no_run
use qubit_local_files::error::{
    LocalFileEffectState, LocalFileError, LocalFileErrorKind, LocalFileOperation,
};

let error = LocalFileError::new(
    LocalFileErrorKind::NotFound,
    LocalFileOperation::Metadata,
);
assert_eq!(error.cause_kind(), Some(LocalFileErrorKind::NotFound));
assert_eq!(error.effect_state(), None);
assert!(!matches!(
    error.effect_state(),
    Some(LocalFileEffectState::Unchanged)
));
# Ok::<(), Box<dyn std::error::Error>>(())
```

`effect_state()` 返回 `None` 表示基础错误没有足够证据推断命名空间副作用；它不表示
`Unchanged`。

Unix 上，默认 feature 与 `test-support` 构建中的 `LocalFileReader::read_vectored` 都使用
文件描述符的原生向量读取路径。Windows 继续使用平台所需的顺序读取回退；如果后续 buffer
读取失败，回退实现会返回已经累计读取的字节数，保持 `Read` 的进度语义。

Rooted metadata 等操作对不含链接的普通路径使用私有的一次顺序目录 cursor。路径包含链接、
缺失组件或原生错误时，会回到既有完整解析器，以保持链接策略和 authority 检查。该实现细节
不改变调用方可依赖的路径与错误契约。

## 排障

| 症状 | 检查方式 |
| --- | --- |
| Rooted 操作拒绝路径 | 检查词法 `..` 或被跟随的链接是否越过虚拟 `/`，以及是否包含 native prefix。虚拟绝对路径、`.` 和未越界的 `..` 都合法；选择 `FollowAcrossScope` 返回 `InvalidOptions`。 |
| 要求保证被拒绝 | 检查所选文件系统的 capability；仅在业务允许时放宽要求。 |
| copy 或 rename 出错 | 先检查类型化失败状态，再决定重试、清理或认定目标不存在。 |
| 临时条目仍存在 | 保留资源并调用显式生命周期方法；drop 清理只是尽力而为。 |

## 限制与最佳实践

Linux、Windows 和 macOS 会进行运行时测试。FreeBSD 与 Android 仅做编译检查。
`capabilities()` 返回所选 authority 的 build capability 快照；Rooted 实例在打开
authority 时缓存该快照。`scope()` 供集成层区分两种命名空间；Rooted 实例的诊断锚点通过
`diagnostic_root()` 单独读取。Host 命名空间的 `limits()` 返回 `SizeLimit::VariesByPath`；使用
`limits_at(path)` 才会针对该路径所在文件系统返回有限值（无法探测时为
`Unknown`）。两个数值限制都必须结合 `length_unit()` 解释：Unix 使用 byte，Windows
使用 UTF-16 code unit，后者不得当成 UTF-8 byte 限制。
原子 rename、原子 replace、尝试临时资源原子持久化的能力、耐久 rename、耐久文件复制、
耐久 writer 发布和耐久临时文件持久化会分别报告，因为各平台对这些完整协议的支持并不相同。
`can_attempt_atomic_temp_persist()` 描述 build 已实现原子尝试协议；是否同一 filesystem 及
运行时 namespace 条件仍由实际操作 outcome 决定。这些 flag 不证明某个具体 mount 或存储
设备已经完成持久化。

本 crate 不会绕过操作系统权限，不默认阻止 mount 或 hard link 边界，也无法消除攻击者可写
目录中的所有跨平台竞争；调用方未设置预算时，库也不承诺应用资源消耗有界。工作区权限隔离
优先使用 Rooted 模式；需要可靠清理的临时资源应放在可信 parent 中；处理不受信任的目录树时
应设置明确预算；操作失败后应检查类型化发布状态；多个线程需要修改同一实例时，由调用方提供
同步机制。

## 延伸阅读

继续阅读 [README](../README.zh_CN.md)、[English user guide](user_guide.md)、
[设计文档](local_file_system_design.zh_CN.md) 或
[API 文档](https://docs.rs/qubit-local-files)。
