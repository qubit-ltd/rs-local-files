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
qubit-local-files = "0.6"
qubit-io = "0.19"
```

启用可选 `async` feature 后，可使用有界本地 I/O worker 和应用持有的 operation manager：

```toml
qubit-local-files = { version = "0.6", features = ["async"] }
qubit-operation = "^0.3"
```

Reader 使用 `qubit_io::ByteInput`，writer 和临时文件使用
`qubit_io::ByteOutput`。这两个标记 trait 会由 `Input<Item = u8>` 和
`Output<Item = u8>` 自动实现；读写方法仍由这两个父 trait 提供。
调用标准 I/O API 时，reader 可转换为 `into_std_read()`；借用 writer 可包装为
`qubit_io::OutputRef::new(&mut writer).into_std_write()`。提交 writer 前先释放借用包装器；
拥有型包装器可通过 `into_inner().commit()` 取回 writer。`flush` 不会发布文件，也不代表
durable 同步。向量 I/O 仍由各文件类型的固有 `read_vectored` 或 `write_vectored` 方法提供。

## 快速开始：发布生成文件

构建工具和导出程序通常只有在全部字节写完后才应替换输出文件。下面的示例创建工作目录，
通过 writer 写入清单并提交，然后读回已经发布的内容。

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
use qubit_io::{Output, Streams};

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{LocalWriteMode, LocalWriteOptions};
use qubit_local_files::outcome::LocalWriterState;

let mut filesystem = LocalFileSystem::host()?;
filesystem.set_default_write_options(LocalWriteOptions::new(
    LocalWriteMode::CreateOrReplace,
))?;

let mut work = filesystem.create_temp_directory()?;
let path = work.path().join("manifest.json");
let mut writer = filesystem.open_writer(&path)?;
writer.write_fully(br#"{"version":1}"#)?;
let outcome = writer.commit()?;
assert_eq!(outcome.state(), LocalWriterState::Committed);

let mut bytes = Vec::new();
Streams::copy_input_to_output(&mut filesystem.open_reader(&path)?, &mut bytes)?;
let content = String::from_utf8(bytes)?;
assert_eq!(content, r#"{"version":1}"#);
work.cleanup()?;
Ok(())
}
```

## 与运行时无关的异步访问

异步 facade 不依赖 Tokio 或其他运行时。先取得 operation outcome，使用并关闭返回资源，
再等待 settlement；持有 reader、writer 或 walker 等待 settlement 可能导致自等待。Rooted
stage 导出需要显式能力；helper 使用克隆的 `File` 时，必须在整个使用期间保留对应 read
lease。清理失败会保留在 settlement/恢复状态中。完整的 outcome、关闭和 settlement 示例见
[用户手册中的异步工作流](doc/user_guide.zh_CN.md#与运行时无关的异步操作)。

## 条件写入：校验 A 的内容后保存 B

编辑器读取文档 A 后，保存时可先检查当前文件的摘要是否仍与 A 一致，再请求发布新内容 B。
此协议不是严格的 compare-and-swap：发布前会检查旧内容，发布后会核验被替换的内容；并发变化仍可能在
B 已短暂可见后才被发现，回滚或恢复也可能失败。
启用可选 `digest` feature，并选择实现 `digest::Digest` 的算法 crate（例如 `sha2` 0.11）；
本 feature 使用 `digest` 0.11。写入器先准备 B，再依据指定条件尝试发布。应用清单中需启用该 feature
并添加算法依赖：

```toml
qubit-local-files = { version = "0.6", features = ["digest"] }
sha2 = "0.11"
```

提交成功会返回已安装 B 的摘要；清理失败通过 `cleanup_errors()` 和
`retained_files()` 报告，但不改变保存成功的结果。提交失败时，根据错误的
`publication()` 状态和保留的恢复文件判断后续检查或恢复步骤；恢复本身失败时，恢复文件可能不完整或不可用。
尚未提交的写入器调用
`abort()` 或离开作用域时会取消，不会发布 B。完整成功和恢复流程及平台限制见
[条件写入指南](doc/user_guide.zh_CN.md#条件写入)，其中包含从 A 检查后发布 B 的完整示例。

## 为什么需要这个项目

如果只执行零散的文件操作，直接使用 `std::fs` 更简单；但当应用还需要稳定的权限边界、
可复用的操作策略、有预算的目录遍历、分阶段发布，或在部分成功后安全恢复时，就必须自行
定义一整套契约。`qubit-local-files` 将这些决策收敛到一个有状态的文件系统对象中，并用
明确的结果类型报告实际状态。

本地文件系统核心仍采用同步实现。可选的 `async` feature 提供与运行时无关的 facade，
通过有界 worker 组执行阻塞文件操作；它不提供内核原生异步文件 I/O。本 crate 不提供
远程存储、provider 注册或应用层授权。

## 提供的能力

| API | 适用场景 |
| --- | --- |
| `LocalFileSystem::host()` | 可复用的进程可见主机命名空间服务。 |
| `LocalFileSystem::rooted(root)` | 以相同操作访问一个已打开目录权限下的后代。 |
| `LocalFileSnapshot` / `read_consistent_with_options` | 从同一已打开文件读取有界字节并保留前后观察；可检测已观察变化，但不提供原子快照。 |
| `LocalDirectoryHandle` | 相对于保留目录权限句柄执行操作，不把显示路径当作原生授权。 |
| `path::LocalFileSystemScope` | 标识实例按主机路径还是 rooted 后代路径解释输入。 |
| `LocalFileWriter` | 分阶段发布后的显式提交或中止。 |
| `LocalDirectoryWalker` | 采用创建时固定策略的惰性目录枚举。 |
| `LocalStagedDirectory` | 填充暂存树、选择性启用验证、以不覆盖已有目标的方式发布，或保留以供恢复。 |
| `directory::LocalTreeEntry` / `options::LocalTreeValidationOptions` | 按精确条目集合验证暂存树，并按需启用链接、特殊文件、硬链接和遍历预算规则。 |
| `LocalTempFile` / `LocalTempDirectory` | 拥有清理责任，并支持 `keep` 与持久化。 |
| `LocalSealedTempFile` | 保留只读句柄的私有清理 owner；`native_path()` 可选且仅适用于 Host。 |
| `AsyncLocalFileSystem` / `LocalIoExecutor` | 可选的运行时无关 facade，通过有界 worker 执行阻塞的本地文件操作。 |
| `path::LocalFileNames` / `path::LocalPaths` | 不丢失 UTF-8 以外文件名信息的原生文件名和词法路径工具。 |

缓存或临时产物要求私有时，应在创建时请求 `LocalPrivacyRequirement::Required`；先公开创建再
收紧权限不满足要求。耐久性按操作定义同步范围：writer 和文件复制同步文件数据及相关命名空间更新，rename 同步命名空间更新。目录树用 `file_sync` 单独报告文件同步，并用 `durability` 表示发布命名空间的要求；`Required` 不代表任意已有树内容都已同步。目录树默认保留合法链接，只应用显式
选择的验证规则，并分别报告 mutation 与 publication。树恢复位置只保存命名空间路径和可选
opaque identity，不含 authority；操作前应通过同一个 parent 重新打开并验证身份。这与 digest
条件写入恢复文件不同，后者由调用方持有且不会自动删除。Windows Rooted `Content`，以及 Host
`Content` 配合 `UseStaging`，会在打开 writer 时返回 `Unsupported`，早于路径解析或创建 staging/parent
副作用；Host `Content` 配合 `PreserveExisting` 和有效的 `Absent` 仍可用。树验证只观测链接条目，不跟随目标或展开 alias
子项。特殊条目默认允许，只有显式启用拒绝规则才会失败。

默认 Host 临时资源创建若失败且残留清理也失败，`LocalFileError` 会保留 `cleanup_error()` 与可供显式重试的
`temp_recovery()` owner；调用方不应只凭路径自行删除。

Reader 和 writer 只接受普通文件；目录和特殊文件会被拒绝，不会被当作字节流打开。

权限观测反映文件条目的原生元数据，不等于综合 ACL、挂载策略等因素后调用者实际拥有的
访问权限。Unix 的 `unix_mode()` 保留观测到的权限位和特殊位；Windows 返回 `None`。
`metadata()` 和目录遍历查看最终链接条目本身，reader 则报告已打开内容句柄的元数据。

`LocalFileSystem` 是有状态的实例 API。Rooted 实例拥有自己的虚拟当前目录；Host
实例只在操作需要绑定相对路径时读取进程当前目录。每个实例还拥有符号链接策略和九种
操作的默认 Options。普通方法使用实例默认值；`open_reader_with_options` 和
`read_prefix_with_options` 使用传入的完整 `LocalReadOptions`。对于
`open_observed_reader`、`read_all_with_options` 和 `read_text_with_options`，存在可选的嵌套
`LocalReadOptions` 时会完整替代 `defaults.read`，缺少时则继承它；snapshot builder 使用自己的
默认读取选项。Snapshot options 从 `LocalReadOptions::new()` 开始，不继承 filesystem defaults；
若要沿用默认读取策略，需显式复制配置。clone 会复制全部可变状态，Rooted clone 只共享不可变的已打开
authority。本 crate 不承诺共享可变配置时的线程安全；调用方可以每线程持有一个 clone，
也可以自行添加同步包装。Host 实例的 `set_current_directory` 修改进程全局当前目录；
Rooted 实例的同名方法只修改该实例。

资源预算均由调用方选择。遍历和复制的深度、条目数、字节数、打开目录数、deadline、
重复名称内存、打开重试时间以及临时名称尝试次数，在调用方显式设置前都不会形成库内隐藏上限。
临时名称冲突时，`None` 表示不限尝试次数。`Some(0)` 无效，创建入口会在产生副作用前返回
`InvalidOptions`。正数包含首次尝试，只有名称冲突会重试。

符号链接策略按 `LocalFileSystem` 实例配置；Rooted 默认
`FollowWithinScope`，Host 默认 `FollowAcrossScope`。Rooted 仅支持
`Reject` 和 `FollowWithinScope`；选择 `FollowAcrossScope` 会返回
`InvalidOptions`。各类操作对最终链接的具体语义请参阅[用户手册](doc/user_guide.zh_CN.md)。
目录 handle 的组件策略只在绑定输入和解析路径时筛选名称；递归列表和复制仍可能发现并复制被拒绝名称。
它不隔离私有子树，也不防护并发重命名。具体边界见[用户手册](doc/user_guide.zh_CN.md)。

调用方可以用 `LocalResolutionLimits` 限制每次路径解析，并用纯校验回调
`LocalResolutionPolicy` 增加规则；子 handle 只能收紧父级限制。安全观测查询已保留的文件对象，
也可以要求文件系统、访问控制和链接数信息。staged tree 可在发布前应用精确的
`LocalOwnerAccessPolicy`。这些 API 不提供原子快照，也不保证不同平台的 ACL 语义完全相同。
详见[解析与安全策略指南](doc/user_guide.zh_CN.md#有界解析与安全策略)及
[英文版](doc/user_guide.md#bounded-resolution-and-security-policy)。

默认发布 staged tree（`file_sync = NotRequired`、保留访问策略，且未配置验证或 owner policy）只检查身份并安装目录，不递归扫描内容。
启用精确验证后，库会按条目数、深度、路径字节数、打开目录数和 deadline 遍历完整目录树；深度上限不会截断验证，
超限即失败。观测条目与 `exact_entries` 配置的预期条目都必须路径唯一，两个集合按“路径 + 类型”比较，不受顺序影响。
发布时将 `file_sync` 设为 `Preferred` 或 `Required` 会遍历并同步普通文件，使用选定的限制；`NotRequired` 跳过该阶段。
`sync_tree_with_options` 则单独遍历并同步完整目录树。非空目录树应用 `LocalOwnerAccessPolicy` 时也要在修改前遍历；
若未设置 `max_entries`，最多保留 256 个子项身份和管理句柄。显式上限会替换此默认值。

Linux/macOS 以外的 Unix 目标尚未实现文件系统安全事实查询：`Optional` 不提供该字段，`Required` 返回 `RequirementNotMet`；其他独立支持的事实仍可用。

条件发布能力依平台而异。Linux 和 macOS 支持有效配置下的 `Absent` 与 `Content`，但具体文件系统仍可能拒绝原生操作。
Windows Rooted `Content` 和 Host `Content` 配合 `UseStaging` 仍返回 `Unsupported`；Windows Host `Content` 配合
`PreserveExisting`，以及 `Absent`，在选项满足要求时仍可尝试。其他 Unix 目标不继承上述支持结论。完整矩阵和恢复边界见
[条件写入指南](doc/user_guide.zh_CN.md#条件写入)。

临时资源清理会校验所有权，但它不是并发同步边界。调用方需要观察清理失败时应显式调用
`cleanup()`；drop 只提供静默、尽力而为的兜底，不报告或记录失败。guard 删除前会比较创建时保存的
文件系统标识，因此通常能拒绝误删替换条目；但标识检查与按路径删除是两个独立操作，
文件系统也可能复用标识。例如，不受信任的并发者删除临时文件后，反复在同一名称上安装新文件，
这不在清理契约的保证范围内。应将临时资源放在并发者无写权的目录，或调用 `keep` 后由上层协调删除。
每个临时资源都有独立的每资源 sandbox 目录结构，但这本身不构成跨平台访问控制保证：Unix
创建 helper 会请求 `0700` mode，Windows 的 `PlatformDefault` 则继承父目录 ACL。需要隐私保证的
调用方应显式请求 `LocalPrivacyRequirement::Required`，并处理创建失败。返回路径包含一个生成的
sandbox 组件；调用 `keep` 会原子发布到生成的 sibling 路径，返回 `LocalPersistOutcome`，并报告
sandbox 清理是否残留。

临时发布失败必须同时检查两个状态轴：`LocalPersistError::state()` 说明本次目标是否发布，
`source_state()` 说明错误产生时的源资源资格。`NotPublished` 可能对应 `Indeterminate`
源，不能单凭目标未发布就重试或删除。`Published + CleanupRequired` 只能清理剩余 sandbox，
不能再次发布或回滚目标。`into_parts()` 返回具名 `LocalPersistErrorParts<T>`；修改保留资源后，
应读取资源自身的实时 `source_state()`。完整恢复示例见[用户手册](doc/user_guide.zh_CN.md#临时资源发布与失败恢复)。

临时目录可通过 `LocalTempCleanupLimits` 设置清理深度、条目数、待处理路径字节数和期限。
限制保存在目录对象中，显式 `cleanup()` 与 Drop 均使用它；每次调用独立计费，Drop 不会退回无界清理。
暂存目录 owner 也会沿用创建时的清理限制，除非调用方显式替换。若旧文件已删除、但恢复容器仍有残留或无法移除，
错误会指向容器路径，不会把已删除文件列为保留资源。
`descendant` 仅接受非空、由正常名称组成的相对路径；`a/b` 合法，`a/../b`、`a/./b` 和空路径均被拒绝。

## 选择合适的权限范围

Host 按原生顺序解释 `.`、`..` 和符号链接；Rooted 使用虚拟路径的词法规范化。
替换写入默认采用 `LocalWriteMetadataPolicy::PreserveExisting`，需要发布 staging 自身的
元数据时显式选择 `UseStaging`；两种策略都保留目标身份检查。list/copy/delete 的
`tighten_resource_limits` 只收紧资源限制，不覆盖操作行为。临时资源的 `persist` 和
`persist_with` 只接受命名空间绝对目标，相对目标使用 `persist_at(base, target, options)`。

主机路径使用 `LocalFileSystem::host()`。当一个已打开目录就是权限边界时，
使用 `LocalFileSystem::rooted(root)`。两种实例提供相同操作，只改变路径解释方式。Host
绝对路径不会读取进程 PWD；Host 相对路径在操作开始时捕获一次进程 PWD，Rooted 相对路径
使用实例的虚拟 PWD。`.` 与空路径表示对应的 PWD；Rooted 对 `..` 进行词法折叠并拒绝
越过虚拟 `/`，Host 则保留原生遍历顺序。在 Rooted 实例中，
`/etc/hosts` 是已打开 root 下的虚拟绝对路径，而不是 Host 的 `/etc/hosts`；native prefix
会被拒绝。

中间符号链接遵循实例策略。Rooted 的绝对链接目标会从虚拟 `/` 重新开始解析，默认
`FollowWithinScope` 会阻止任何链接越出已打开的 authority。`FollowAcrossScope` 仅适用于
Host，Rooted 会拒绝该配置。之后重命名诊断用 root 路径也不会重定向已打开的 authority。
公共资源路径和错误路径统一使用可再次传入同一实例的 namespace-absolute 身份；底层物理
路径仅在可获得时作为可选诊断信息提供。
Windows 上，Rooted 的链接读取、链接类型判断和链接创建都相对于已打开 handle 执行。
复制悬空链接或目标位于 Rooted authority 外的链接时，不会查看或打开链接目标。

复制会根据源元数据选择文件或目录行为。复制和重命名失败会保留已证实的最强发布状态，
因此调用方必须检查类型化失败，不能假设出错后目标未变。`CreateNew` 和
`CreateOrReplace` 在目标目录中暂存；`Append` 会直接写入已有普通文件，不能满足要求的
原子性。

复制模式明确区分单个 `Entry`（普通文件或最终链接）、目录树 `Tree` 和自动判断 `Auto`。
详见[源模式契约](doc/user_guide.zh_CN.md#复制源模式)。目录树复制会在修改前拒绝相同或互为祖先的源、目标目录；
替换目录树时应先复制到无祖先重叠的兄弟路径。即使叶子条目被跳过，目录权限变更仍属于复制副作用。
Host 目录复制预检可能跟随已有的最终目标链接来判断重叠。例如，`container/source` 与
`alias -> container` 并存时，将前者复制到 `alias` 会在修改前以 `InvalidPath`/`Unchanged` 拒绝。
这是保守的预检行为，不表示发布会跟随链接并写入 referent。

目录遍历采用惰性读取。非递归列表会直接返回悬空链接条目，不解析它的目标。递归遍历会报告
目录链接导致的错误；`Continue` 会返回局部子目录打开错误，并继续处理安全的同层条目。全局预算
耗尽时遍历仍会终止。

## 平台范围

CI 配置了 Linux、Windows 和 macOS 运行时测试；本次修改是否通过，以对应 CI 结果为准。
Linux 上的 CI 会交叉编译 Android `armv7-linux-androideabi`，不运行 Android 代码。只有具备原生 Android shell
和可写临时目录时，才会运行独立且范围有限的 Android 原生测试套件。两类检查的配置见
`.infra/platform/platform.toml` 和 `.github/workflows/ci.yml`。Windows 暂存清理和 sealed cleanup 在历史 revision
[`251e103bd9c60bf749e60453e688cf9ac4bde6f2`](https://github.com/qubit-ltd/rs-local-files/commit/251e103bd9c60bf749e60453e688cf9ac4bde6f2)
有 Windows Server 2025 NTFS 原生记录：默认及 `--all-features` 配置下各 3 项暂存测试通过；所选 sealed cleanup 测试在默认配置下为 7/7、在 `--all-features` 配置下为 11/11。该记录只覆盖此 revision 与所列测试，实施基线 `c5639417080648da263216f698893c3a6c3c7f2d` 尚无对应原生覆盖；其他条件打开和私有资源行为也不在该记录范围内。
详见[Windows 暂存与清理证据](doc/review-rlf4-windows-staging-cleanup-evidence.md)及独立的[Windows 枚举证据](doc/review-rlf3-windows-evidence.md)。所引 run 的两个 Windows job 成功，但整个 workflow 因 coverage 门槛失败。
本 crate CI 只验证当前 crate；adapter 兼容性由 adapter 仓库或独立集成 job 负责。FreeBSD 暂不支持：当前原生检查只覆盖编译和少量兼容性合同，
不再运行完整行为测试套件。各 API 失败用例及原因见 [FreeBSD 兼容性说明](doc/freebsd-compatibility.zh_CN.md)。Android 不在条件发布协议支持矩阵中；上述 ARMv7 编译和有限原生测试不构成 Android 通用协议支持。
本 crate 不承诺这些目标上的运行时保证。`capabilities()` 只报告当前 build 是否实现了
对应的完整操作协议，不会探测具体的运行时文件系统，也不声称证明底层硬件已经持久化数据；无法满足要求的原子性或耐久性时，
会在命名空间变更前拒绝操作。条件写入支持范围取决于平台和 authority：

| 平台 | Host | Rooted |
| --- | --- | --- |
| Linux / macOS | `Absent`、`Content` | `Absent`、`Content` |
| Windows | `Absent`；`Content` 需使用 `PreserveExisting` 元数据策略 | `Absent`；`Content` 不支持 |
| FreeBSD 14.3 | 暂不支持 | 暂不支持 |

所选操作协议无法满足 Required 原子性或耐久性时，会在命名空间变更前拒绝操作。
原子 rename、原子 replace、尝试临时资源原子持久化的能力、耐久 rename、耐久文件复制、
耐久 writer 发布和耐久临时文件持久化均独立报告，因为各平台对这些协议的支持不同。
`can_attempt_atomic_temp_persist()` 只描述实现能力，不承诺任意 source/target 都能原子完成；
实际结果以操作 outcome 为准。
路径限制观测始终携带单位：Unix 使用 byte，Windows 使用 UTF-16 code unit。handle-relative
命名空间没有可证明的固定整路径上限时，Windows 的整路径限制保持 `Unknown`；`space_at()`
与组件限制从所选文件系统 handle 查询。Windows Host 路径转换明确不支持 UNC 路径。

## 延伸阅读

- [User guide](doc/user_guide.md)
- [FreeBSD 兼容性说明](doc/freebsd-compatibility.zh_CN.md)
- [同步并发布完整暂存目录树](doc/user_guide.zh_CN.md#完整树同步后再耐久发布)
- [观测要求与累计计数](doc/user_guide.zh_CN.md#观测要求与累计计数)
- [用户手册](doc/user_guide.zh_CN.md)
- [API 文档](https://docs.rs/qubit-local-files)
- [English design document](doc/local_file_system_design.md)
- [本地文件系统设计文档](doc/local_file_system_design.zh_CN.md)
- [English README](README.md)

## 测试

```bash
# 使用默认 feature 集运行测试
cargo test

# 使用项目声明的全部 feature 运行测试
cargo test --all-features

# 运行项目 CI 检查
./.infra/bin/ci-check.sh

# 检查代码覆盖率
./.infra/bin/coverage.sh

# 生成带本地运行来源信息的覆盖率报告
./scripts/run-coverage-with-provenance.sh
```

`.infra` 与 `scripts` 下的命令应在源码 checkout 中运行；它们不包含在已发布的 crate 包中。

wrapper 会在采集开始和结束时检查项目输入；观察到状态变化就会失败。这不是不可变构建快照，
无法保证发现瞬时修改，也不检查外部依赖的内容。失败后应停止修改、固定输入并重新运行。共享 CI
的覆盖率产物仍附属于对应 workflow run，wrapper 不会替代这些产物。

## 许可证

Copyright (c) 2025 - 2026. Haixing Hu. All rights reserved.

本项目基于 Apache License 2.0 授权。完整许可证文本请参阅
[LICENSE](LICENSE)。

## 贡献

欢迎贡献。请遵循 Rust API 指南，及时更新公共 API 文档与测试，并在提交
Pull Request 前运行 `./.infra/bin/align-ci.sh`格式化代码，运行`./.infra/bin/ci-check.sh`对齐CI要求。

## 作者

**Haixing Hu** - *Qubit Co. Ltd.*

仓库地址：[https://github.com/qubit-ltd/rs-local-files](https://github.com/qubit-ltd/rs-local-files)
