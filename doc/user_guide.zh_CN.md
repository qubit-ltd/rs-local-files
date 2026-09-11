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
最强目标状态：`Unchanged` 表示本次操作没有修改任何目标条目，`Indeterminate` 表示无法确定
最终目标状态。即使打开读取器失败，`read_prefix` 错误仍保留外层的 `Read` 操作。

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

权限观测反映文件条目的原生元数据，不等于综合 ACL、挂载策略等因素后调用者实际拥有的
访问权限。Unix 的 `unix_mode()` 保留观测到的权限位和特殊位；Windows 返回 `None`。
`metadata()` 和目录遍历查看最终链接条目本身，reader 则报告已打开内容句柄的元数据。

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
源模式判断不跟随最终链接；递归目录树内部遇到的目录链接仍按有效遍历策略处理。
源模式不会改变中间链接解析或目录树的链接遍历语义。

目录树复制无法提供强制原子性或持久性，链接复制无法提供强制原子性。请求无法满足的
`Required` 保证时，会在修改目标前返回 `RequirementNotMet`。链接持久性取决于平台能力，
以及目标父目录和新建祖先目录的同步结果。
Windows 复制链接会保留源链接的文件/目录类型，包括悬空链接；覆盖目标链接时不会删除
该链接指向的内容。

## 遍历和临时资源

`LocalFileSystem::list` 返回惰性的 `LocalDirectoryWalker`。它按需打开和推进目录；
绑定后的命名空间 root、Options、符号链接策略、PWD snapshot 和 authority 在创建时固定。默认不设置
深度、条目数、名称内存、deadline 或打开目录数预算。调用方设置打开目录预算后，
`Reopen` 会按需关闭并重新打开活动 frame，`Fail` 则会在边界返回 `ResourceLimit`。
零句柄预算无效并返回 `InvalidOptions`。Rooted 会逐项读取目录，避免先收集到 `Vec`；
drop walker 只释放句柄。Host 列举即使跟随符号链接进入另一个物理目录，也会保留请求的
namespace 路径作为公开 root；可选的 diagnostic path 仍可记录实际访问的物理路径。

临时文件和目录按当前源资格承担清理责任。每个资源都创建在独立的私有 sandbox 中，
sandbox 会和资源一起清理。需要观察清理失败时应显式调用 `cleanup()`；drop 只会静默地尽力清理。
`keep` 会原子发布到 sandbox 外生成的 sibling 路径，返回 `LocalPersistOutcome`，其 cleanup state
会报告 sandbox 残留。未显式指定 parent 时，在该次操作捕获的 filesystem PWD 下创建。
`path()`、`keep` 和持久化结果对 Host 与 Rooted 都返回 namespace-absolute 路径，因此
后续 PWD 即使变化，也能把它们再次传给同一个 filesystem。持久化失败会保留资源，后续允许
的操作由发布状态和源资格共同决定。创建前会校验前缀和后缀：原生分隔符、NUL 与便携保留名称
不会留下条目。除非调用方设置 `max_attempts`，名称冲突尝试次数没有上限。

## 临时资源发布与失败恢复

临时文件写完后才能发布。`persist` 和 `persist_with` 消耗 guard，接收命名空间绝对目标；
相对目标通过 `persist_at` 明确指定基准目录。`LocalPersistOptions::new()` 遇到已有目标会失败。
成功时返回 `LocalPersistOutcome`，调用方应检查路径、原子性、耐久性、`cleanup_state()` 和
`cleanup_error()`。sandbox 残留通过成功结果报告，不会撤销发布，也不表示可以再次发布。

失败时要分别检查目标发布结果与源资源资格：

| 查询 | 含义 |
| --- | --- |
| `failure.state()` | 本次目标发布结论：`NotPublished`、`Published` 或 `Indeterminate`。 |
| `failure.source_state()` | 错误产生时的源资格快照：`Owned`、`CleanupRequired`、`Released` 或 `Indeterminate`。 |
| `failure.resource().source_state()` | 保留资源的实时资格，包含通过 `resource_mut()` 执行操作后的变化。 |

`Owned` 允许继续执行生命周期操作，但每次仍须复核身份；它不保证内容完整，递归清理失败前
可能已经删掉部分后代。`CleanupRequired` 表示原条目已离开源位置，guard 只负责私有 sandbox，
只能重试 sandbox 清理。`Released` 没有剩余责任，重复 `cleanup()` 幂等成功。
`Indeterminate` 表示无法证明源操作资格，persist、keep、cleanup 和 Drop 删除均被禁止。

| 失败场景 | 发布状态 | 源资格 | 恢复方式 |
| --- | --- | --- | --- |
| Owned 资源的目标参数或父目录准备失败 | `NotPublished` | `Owned` | 修正目标后重试，或 keep、cleanup。 |
| 源身份有效，no-replace 遇到目标冲突 | `NotPublished` | `Owned` | 换目标、明确选择覆盖，或清理源。 |
| 发布前发现原源条目被替换 | `NotPublished` | `Indeterminate` | 仅诊断，不能删除替换条目。 |
| 无法确定 native install 是否生效 | `Indeterminate` | `Indeterminate` | 由外部核对；不得自动删除或恢复资格。 |
| 安装成功，随后文件发布同步失败 | `Published` | `CleanupRequired` | 保留已发布目标，只清理 sandbox。 |
| 对 `CleanupRequired`、`Released` 或 `Indeterminate` 再次发布 | `NotPublished` | 原源状态 | 拒绝本次发布，保留源限制与之前已发布目标的事实。 |

生命周期资格检查先于新目标解析。再次传入错误目标不能让 Indeterminate 资源变回 Owned。
错误只描述本次调用，后来被拒绝的调用不会让先前已发布目标消失。`keep` 遵循同一源资格契约。
目录 guard 无法证明任意后代都已同步，因此要求耐久性时会在发布前拒绝；文件则可能在安装
目标后的父目录同步阶段失败，本库不会回滚已发布目标。

### 从输出名称冲突中恢复

下面在独立临时 parent 中暂存清单，利用 no-replace 冲突稳定地产生失败，再显式检查清理结果。
已有输出的内容保持不变。需要重试时，先确认保留资源当前仍为 `Owned`。

```rust
use std::io::Write;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalTempSourceState;

let filesystem = LocalFileSystem::host()?;
let parent_options = LocalTempDirectoryOptions::new()
    .with_parent(&std::env::temp_dir())
    .with_max_attempts(16);
let mut parent = filesystem.create_temp_directory_with_options(&parent_options)?;
let target = parent.path().join("manifest.json");
std::fs::write(&target, b"existing manifest")?;
let options = LocalTempFileOptions::new().with_parent(parent.path());
let mut temporary = filesystem.create_temp_file_with_options(&options)?;
temporary.write_all(br#"{"complete":true}"#)?;

let failure = temporary.persist(&target).expect_err("no-replace must reject an existing target");
assert_eq!(failure.state(), LocalPersistFailureState::NotPublished);
assert_eq!(failure.source_state(), LocalTempSourceState::Owned);
let mut parts = failure.into_parts();
assert_eq!(parts.state, LocalPersistFailureState::NotPublished);
assert_eq!(parts.source_state, LocalTempSourceState::Owned);
parts.resource.cleanup()?;
assert_eq!(parts.resource.source_state(), LocalTempSourceState::Released);
assert_eq!(parts.source_state, LocalTempSourceState::Owned);
assert_eq!(std::fs::read(&target)?, b"existing manifest");
parent.cleanup()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`into_parts()` 返回 `error::LocalPersistErrorParts<T>`，字段为 `error`、`resource`、
`requested_target`、`resolved_target`、`stage`、`state` 和 `source_state`。两个状态字段
都是错误快照；示例清理后，资源实时状态为 Released，快照仍为 Owned。
丢弃错误或 parts 也会丢弃其拥有的资源，仍受相同的源资格与清理限制约束；不能只看到
`NotPublished` 就忽略保留资源。

## 临时目录清理限制

创建目录时，用 `LocalTempDirectoryOptions::with_cleanup_limits` 保存
`options::LocalTempCleanupLimits`。四项限制为 `max_depth`、`max_entries`、
`max_pending_path_bytes` 和 `deadline: Duration`，每项都有 getter、`with_*` 和
`without_*`。`new()` 与 `Default` 均不限；该类型不包含 recursive 或 missing-ok 行为开关。

```rust
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;

let filesystem = LocalFileSystem::host()?;
let limits = LocalTempCleanupLimits::new()
    .with_max_depth(8)
    .with_max_entries(1_024)
    .with_max_pending_path_bytes(1024 * 1024)
    .with_deadline(Duration::from_secs(30));
let options = LocalTempDirectoryOptions::new()
    .with_parent(&std::env::temp_dir())
    .with_cleanup_limits(limits);
let mut directory = filesystem.create_temp_directory_with_options(&options)?;
assert_eq!(directory.cleanup_limits(), limits);
assert!(directory.descendant(Path::new("a/b")).is_ok());
assert!(directory.descendant(Path::new("a/../b")).is_err());
assert!(directory.descendant(Path::new("a/./b")).is_err());
assert!(directory.descendant(Path::new("")).is_err());
directory.cleanup()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`cleanup_limits()` 返回资源保存的限制，`set_cleanup_limits(limits)` 不执行 I/O，影响之后的
显式 cleanup 与 Drop。每次调用都按同一组已存限制开启新预算。显式清理失败后，Drop 最多
再尝试一次，不会退回无限制清理；剩余工作需要更多预算时，调用方必须明确修改限制。
deadline 是每次调用的协作式时长，不是对象生命期累计额度，也不能中断已经阻塞的原生 I/O。

源根深度为 0，并计为一个条目；后代在加入队列前占用条目额度。待处理路径字节数只统计
工作队列保留的原生编码路径，不包括分配器开销、reader 缓冲或当前枚举对象。
零值与非法组合沿用 `LocalDeleteOptions` 的校验规则，任何删除前先完成校验。
sandbox 释放最多额外执行一次原生删除，不计入源树条目和路径预算；但仍使用同一次调用的
期限。计时从 cleanup 入口开始，树删除完成后不重启，删除 sandbox 前再次检查相同 deadline。

清理复用递归删除的后序遍历，不进行字典序排序。子符号链接只删除链接自身，不访问目标；
库不保证删除顺序或常量内存。部分子项删除后失败，会保留部分副作用与准确失败路径；只要
源树仍归 guard 所有，源资格就保持 `Owned`。再次 cleanup 处理剩余树；若选择发布，发布的
也只是剩余内容。源实体已删除而 sandbox 删除失败时进入 `CleanupRequired`，重试只处理
sandbox。并发新增子项可能导致 `DirectoryNotEmpty`，操作会返回错误，不会无限重扫。

`child` 接受单个正常名称；`descendant` 只接受非空、全部由正常名称组成的相对路径，拒绝根、
原生 prefix/盘符、字面 `.`、`..` 和 NUL。即使归一化后仍在目录内，`a/../b` 与字面
`a/./b` 也会失败。这些方法只构造路径，不执行 I/O，不赋予 Rooted 权限，也不证明磁盘上
符号链接的目标安全。它们的严格规则与普通 Rooted 路径归一化、普通 Host 原生解析分别适用。

## Host 路径、替换元数据与资源限制

### Host 路径保留原生解析顺序

Host 将相对路径绑定到该次操作的进程 PWD，保留点组件和目录意图。以 Unix 上的
`a/link -> ../b/inner` 为例，Host 读取 `a/link/../config` 会访问 `b/config`；
Rooted 对调用者路径进行词法折叠，访问的则是 `a/config`。若 `missing` 不存在，
Host 的 `missing/../config` 会失败，不能跳过缺失组件。Rooted 拒绝越过虚拟 `/` 的
词法路径；Host 按原生规则处理根目录，包括 Unix 的 `/..`。Windows 的 `C:foo` 等
drive-relative 输入仍然无效。Host 不提供 Rooted 的隔离边界。

默认 Host metadata 只查询最终路径，不再逐级探测所有前缀；显式 `Reject` 仍检查经过的
链接，包括 `link/..`。可用 `cargo bench --bench local_files -- deep_metadata` 在同一
fixture 上对照 std、Host 和 Rooted；耗时受文件系统与路径深度影响。

### 显式选择替换元数据策略

`LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)` 默认使用
`LocalWriteMetadataPolicy::PreserveExisting`。需要保留 staging 自身元数据时，选择
`UseStaging`。这可能改变目标的访问控制；staging 创建时仍可能从原生环境继承权限。

| 平台与范围 | PreserveExisting | UseStaging |
| --- | --- | --- |
| Unix Host/Rooted | 保留已有实现支持的 owner/mode/ACL/xattr 等元数据，复制失败会报错 | 不请求读取旧文件内容或复制旧元数据 |
| Windows Host | 使用 `ReplaceFileW` 原生元数据合并 | 使用不合并元数据的原生替换 |
| Windows Rooted | 只保留 portable permissions，不承诺完整 ACL/owner 保留 | 跳过 portable permissions 复制 |

两种策略都检查目标类型和身份，但检查与安装不构成原子 compare-and-swap。
CreateNew 没有旧元数据可复制；Append 在两种策略下都直接追加。元数据保留失败发生在
发布前，父目录同步失败则可能发生在发布后；重试前应检查 commit 错误及其保留的状态。

```rust
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
    .with_metadata_policy(LocalWriteMetadataPolicy::UseStaging);
assert_eq!(options.metadata_policy(), LocalWriteMetadataPolicy::UseStaging);
```

### 收紧预算时保留请求行为

list/copy/delete options 提供 `tighten_resource_limits(self, ceilings: &Self)`。
每项限制中，`None` 表示无上限；两侧都有值时取较小者。0 仍是 0，操作入口仍会拒绝
非法的零句柄上限。deadline 仍从操作开始计时。递归、覆盖、创建父目录等行为全部来自
接收者；`*_with_options` 完整使用传入 options 的契约不变。

```rust
use qubit_local_files::options::LocalCopyOptions;

let requested = LocalCopyOptions::new().with_max_bytes(100);
let ceilings = LocalCopyOptions::new().with_max_bytes(10);
let effective = requested.tighten_resource_limits(&ceilings);
assert_eq!(effective.max_bytes(), Some(10));
```

`qubit-fs-local` 的 provider list 上限统计前缀过滤前的原生条目，请求上限统计过滤后
返回的条目。因此，即使没有匹配项，也可能耗尽 provider 上限。这些限制按单次操作计算，
不是跨并发请求的累计配额。

### 用明确基准目录发布临时资源

两种临时资源的 `persist` 和 `persist_with` 只接受命名空间绝对目标。
相对目标使用 `persist_at`：

```rust,no_run
use std::io::Write;
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;

let filesystem = LocalFileSystem::host()?;
let base = std::fs::canonicalize(std::env::temp_dir())?;
let mut temporary = filesystem.create_temp_file()?;
temporary.write_all(b"generated report")?;
let published = temporary.persist_at(
    &base, Path::new("report.txt"), LocalPersistOptions::new(),
)?;
# let _ = published;
# Ok::<(), Box<dyn std::error::Error>>(())
```

base 必须是命名空间内已存在的绝对目录，不能显式包含 `.`/`..`；target 必须非空且相对，
不能带 root 或 native prefix。target 的 parent component 按 Host 原生或 Rooted 词法
规则处理。base 不是新沙箱：始终使用创建时捕获的 authority 和链接策略，Rooted 的诊断
目录改名也不改变权限。Rooted `/` 不能成为最终发布目标。在确认资源仍允许发布后，参数无效时会在同步或关闭源、
创建父目录之前返回 `ResolveTarget` 并保留 guard；后续阶段失败时文件可能已经关闭，
应检查发布状态。创建时 PWD 只用于诊断，之后修改进程 PWD 不影响显式目标基准。
`keep()` 仍生成绝对 sibling 目标。发布到固定名称时，应按应用需求选择冲突策略。

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

`LocalPersistError` 同时保留临时资源和结构化的 `LocalFileError`；其 `state()` 报告本次发布，`source_state()` 保存源资格快照；
完整恢复须同时检查两者，操作资源后再读取其当前 `source_state()`。存在原生 I/O 错误时，可从结构化错误的 source 取得。

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

CI 配置了 Linux、Windows 和 macOS 运行时测试；具体修改的验证状态以实际 CI 结果为准。
FreeBSD 与 Android 仅做编译检查。
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

## 测试与性能基线

项目使用 registry 依赖，`Cargo.lock` 固定解析版本与校验和。下游契约脚本以 `--locked`
检查协调的 local-files、fs、fs-local 和 mime 依赖闭包。性能测量在 correctness tests 之外执行：

```bash
# 编译 Criterion benchmark
cargo bench --locked --bench local_files --no-run

# 同一 fixture 对照 std、Host 和 Rooted metadata
cargo bench --locked --bench local_files -- deep_metadata

# 测量新建/替换写入以及宽树/深树临时目录清理
cargo bench --locked --bench local_files -- '^(writer_scenarios|temp_directory_cleanup)/' --sample-size 20 --warm-up-time 1 --measurement-time 2
```

比较 benchmark 结果时，应使用同一 harness、机器、文件系统、工具链和构建 profile。
记录替换写入的 Host/Rooted、新建/已有目标、元数据策略、耐久性和 payload 组合，以及宽树、
深树清理的无限制/显式限制组合。fixture 创建与最终 scratch parent 回收应在计时区外；
平台不支持的策略须标记为不支持，不能充当成功样本。Criterion 结果用于评估趋势与区间，
不作为 CI 墙钟阈值，也不代表已经证明性能提升。清理工作量和队列路径内存随目录树变化。

写入 ID 使用 `writer_scenarios/{scope}/{target}/{metadata}/{durability}/{size}`；
清理 ID 使用 `temp_directory_cleanup/{scope}/{shape}/{limits}`。

## 延伸阅读

继续阅读 [README](../README.zh_CN.md)、[English user guide](user_guide.md)、
[设计文档](local_file_system_design.zh_CN.md) 或
[API 文档](https://docs.rs/qubit-local-files)。
