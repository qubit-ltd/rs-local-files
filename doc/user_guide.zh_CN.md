# Qubit Local Files 用户手册

[English](user_guide.md) · [README](../README.zh_CN.md) ·
[设计文档](local_file_system_design.zh_CN.md) ·
[API 文档](https://docs.rs/qubit-local-files)

## 手册目标与读者

本手册面向 Rust 1.94 及以上版本的 `qubit-local-files` 0.5.0 使用者，适用于直接操作主机
文件系统，或需要把操作限制在一个已打开目录之下的应用。它不是 provider 注册表、远程
文件系统 API，也不替代 provider 层的逻辑路径模型。本 crate 提供同步 API；异步应用应在
合适的 blocking 执行环境中调用。

## 操作契约

`LocalFileSystem` 会在解析路径或修改文件系统前校验静态选项组合。复制失败会报告已证明的
最强目标状态：`Unchanged` 表示本次操作没有修改任何目标条目，`Indeterminate` 表示无法确定
最终目标状态。即使打开读取器失败，`read_prefix` 错误仍保留外层的 `Read` 操作。
`read_all`、`read_text` 和一致性读取的结构化错误会保留解析后的命名空间路径；相对路径还会保留
本次请求使用的 PWD 快照。

## 概念模型

```text
Host 命名空间 ── LocalFileSystem::host() ── 操作时读取进程 PWD
已打开根目录 ─── LocalFileSystem::rooted(root) ── 虚拟 / 与实例 PWD
```

## 快照、固定目录与私有临时数据

编辑器可以用 `read_text` 读取 UTF-8 文本，或用
`read_consistent_with_options` 获取有字节上限的 `LocalFileSnapshot`。快照在同一个已打开
文件句柄上读取前后观察；身份、大小或可用的原生时间戳变化会返回
`ChangedDuringRead`。这能检测已观察到的变化，但无法对抗可并发写入的攻击者，也不提供原子
快照。编辑时应以返回的字节作为文档版本；若发布更新时必须确认原版本仍有效，再使用条件发布。

`LocalFileSystem::host()` 在进程可见的 Host 命名空间解释路径。
`LocalFileSystem::rooted(root)` 和 `open_directory_with_options` 保留已打开目录权限句柄，
其子路径使用该句柄下的命名空间相对名称。公开的 `path()` 是命名空间坐标，不是原生授权令牌。
尤其 Rooted 路径是虚拟路径：应用应在 wire 格式边界自行转换，不能把它交给原生 API。
sealed 临时文件的 `native_path()` 仅对 Host 资源可用；它只是路径观察，如果其他参与者能重命名
条目，就不代表永久绑定。

共享缓存或生成产物需要创建时保证权限时，应在创建临时文件或目录时请求
`LocalPrivacyRequirement::Required`。该保证在创建过程中检查；先创建公开对象再收紧权限不满足
要求。临时文件可使用 `LocalSealOptions` seal；返回的 `LocalSealedTempFile` 保留只读句柄用于观察
和清理，但不公开字节流 reader 或可变原生句柄。需要读取字节时，应通过同一 filesystem authority
重新打开它的命名空间路径。同步文件数据不代表后续 rename 已耐久。`Required` 的同步范围由操作决定：writer 和文件复制同步文件数据及相关命名空间更新；rename 同步命名空间更新。目录树发布分别用 `file_sync` 表示树中文件的同步要求，用 `durability` 表示发布命名空间的耐久性要求；前者与后者相互独立，`Required` 也不表示任意已有树内容都已同步。

创建目录备份时，可创建 `LocalStagedDirectory`，通过其固定目录句柄填充，并只在需要时显式
启用验证规则。目录树验证可以拒绝链接、特殊条目或树外硬链接，也可以设置明确的资源预算；
这些规则彼此独立。默认会保留合法链接而不会跟随链接。发布会消费 stage 并使用 no-replace
语义。错误时分别检查 mutation 与 publication 状态。保留的树恢复位置只包含命名空间路径和可选
opaque identity，不含权限句柄；重新操作前须通过同一个 parent 重新打开并验证身份。
显式 `retain_in_place` 只转移责任，不重命名也不发布。
`apply_access_policy` 是独立的 `&mut` 操作：`Preserve` 不执行原生修改；另外两种当前用户策略
只修改经过 identity 复核的文件和目录。如果前面的条目已修改、后续某项失败，应检查错误中的
`access_state()`；`Mixed` 表示权限只转换了一部分，stage 仍由调用方持有。库不会声称整棵树可回滚。

### 为发布选择一种最终访问策略

`LocalTreePublishOptions` 分别保存常规访问策略和精确的
`LocalOwnerAccessPolicy`。两个 builder 只记录各自的值，不会替你清除另一个字段。可用组合如下：

| Owner policy | `access` | 结果 |
| --- | --- | --- |
| None | 任意受支持的值 | 按所选访问策略处理 |
| Some(policy) | `Preserve` | 应用精确 owner policy，仍受平台能力和值校验约束 |
| Some(policy) | 非 `Preserve` | 在开始修改前返回 `InvalidOptions`；不发布并交还 stage |

冲突由 `publish_new` 检查，构造 options 时不会报错。下面的函数展示如何读取失败状态并显式清理 stage；调用时传入像前面的目录备份示例那样打开的 `parent`：

```rust
use std::ffi::OsStr;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::{LocalTempDirectoryOptions, LocalTreePublishOptions};
use qubit_local_files::outcome::{LocalTreeMutationState, LocalTreePublication};
use qubit_local_files::policy::{LocalOwnerAccessPolicy, LocalTreeFinalAccess};

fn reject_conflicting_publish(parent: &LocalDirectoryHandle) -> Result<(), Box<dyn std::error::Error>> {
    let stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
    let options = LocalTreePublishOptions::new()
        .with_owner_access_policy(LocalOwnerAccessPolicy::new())
        .with_access(LocalTreeFinalAccess::CurrentUserOnly);
    let failure = stage.publish_new(parent, OsStr::new("backup-conflict"), &options)
        .expect_err("两种访问策略不能同时指定");
    assert_eq!(failure.cause().kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(failure.mutation_state(), LocalTreeMutationState::NotStarted);
    assert_eq!(failure.publication(), LocalTreePublication::NotPublished);
    let (_, stage, retained) = failure.into_parts();
    assert!(retained.is_empty());
    stage.expect("未发布的 stage 会交还给调用方").cleanup()?;
    Ok(())
}
```

普通发布通常保留默认的 `LocalTreeFinalAccess::Preserve`，只有确实需要精确所有者或访问设置时才配置 owner policy。遇到冲突时，应先查看状态，再消费错误；取回的 stage 可以显式清理，也可以用 `retain_in_place()` 转移后续处理责任。

### 从已打开对象读取平台访问事实

诊断或审计需要原生访问控制事实时，应通过同一个 `LocalFileSystem` authority 打开文件，并从保留句柄的 reader 发起查询：

```rust
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::observation::{LocalObservationRequirement, LocalSecurityObservationOptions};
use qubit_local_files::options::LocalObservedReadOptions;

fn inspect_access(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let filesystem = LocalFileSystem::rooted(root)?;
    let reader = filesystem.open_observed_reader(
        Path::new("artifact.bin"), &LocalObservedReadOptions::new(),
    )?;
    let facts = reader.security_observation(
        &LocalSecurityObservationOptions::new()
            .with_access(LocalObservationRequirement::Optional),
    )?;
    if let Some(access) = facts.access() {
        if let Some(unix) = access.unix() {
            let _observed_unix = unix;
        }
        if let Some(windows) = access.windows() {
            let _observed_windows = windows;
        }
    }
    Ok(())
}
```

`None` 表示当前平台无法提供所请求的事实；分别检查两个 accessor，不要根据序列化数据猜测主机平台。`Optional` 仍会实际查询，因此权限拒绝、I/O、数据格式错误或身份变化仍可能作为错误返回。这些值只是观察到的事实，不能证明后续操作一定获得授权。多个原生字段分开查询，也不构成原子快照。

创建 stage 时会记录新目录的 identity，并在接管前再次核对。Linux/Unix 竞态测试确认：若创建期间随机名称被替换，操作会拒绝接管替换目录。若其他参与者仍能修改父目录，后续按名称操作依然可能发生竞态。

`verify_entries()` 会把观测到的条目与 expected 集合精确比较；expected 集合应包含链接条目本身，
不要把通过 alias 可见的后代也加入集合。重复 expected path 返回 `InvalidOptions`。设置有限深度时，
验证仍覆盖整棵树；发现超出深度的条目会报错，不会静默截断。expected path 字节在建立索引前计入预算，
验证与遍历共同受条目数、深度、路径字节、打开目录数和 deadline 限制。特殊条目默认允许，只有显式
启用拒绝规则才会失败。观测能发现已访问条目的变化，但不构成原子快照。发布未附带验证选项时不会递归扫描。
迁移时，精确 expected 集合只列实际条目（链接本身也算一个条目），不要列出通过链接 alias 才能到达的后代。
正确使用普通文件 reader 和临时资源的代码无需调整，也不需要新增树验证选项。

### 完整树同步后再耐久发布

先完成 stage 写入和最终权限设置，再执行同步。`sync_tree_with_options` 依次同步普通文件、从深到浅的子目录、stage 根目录和创建它的 parent。期限采用协作式检查：每个操作间会检查期限，但无法取消已经进入阻塞状态的内核调用。遍历会逐步观察条目，不构成抵抗并发修改的原子快照。

树同步成功后，发布前不要再修改文件内容、名称或权限。任何此类修改都需要重新同步受影响的文件或目录。下面的 Unix 示例在独立临时目录中创建备份、同步整棵树，然后请求以耐久 rename 发布：

```rust
#[cfg(unix)]
{
use std::ffi::OsStr;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::options::{
    LocalDirectoryOpenOptions, LocalListOptions, LocalTempDirectoryOptions,
    LocalTreePublishOptions,
};
use qubit_local_files::outcome::LocalTreePublication;
use qubit_local_files::policy::LocalDurabilityRequirement;

fn publish_backup(parent: &LocalDirectoryHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
    let mut manifest = &b"backup version 1\n"[..];
    stage.write_file_new(Path::new("manifest.txt"), &mut manifest, 1024)?;

    if let Err(error) = stage.sync_tree_with_options(parent, &LocalListOptions::new()) {
        stage.cleanup()?;
        return Err(error.into());
    }

    let options = LocalTreePublishOptions::new()
        .with_durability(LocalDurabilityRequirement::Required);
    match stage.publish_new(parent, OsStr::new("backup-current"), &options) {
        Ok(outcome) => {
            assert_eq!(outcome.publication(), LocalTreePublication::Published);
            assert!(outcome.durable());
        }
        Err(error) => {
            let publication = error.publication();
            let (cause, stage, retained) = error.into_parts();
            if publication == LocalTreePublication::NotPublished {
                if let Some(stage) = stage {
                    stage.cleanup()?;
                }
                for resource in retained {
                    resource.cleanup()?;
                }
            } else {
                if let Some(stage) = stage {
                    drop(stage.retain_in_place());
                }
                for resource in retained {
                    drop(resource.retain_in_place());
                }
            }
            return Err(cause.into());
        }
    }
    Ok(())
}

let result = (|| -> Result<(), Box<dyn std::error::Error>> {
    let fixture = tempfile::tempdir()?;
    let filesystem = LocalFileSystem::host()?;
    let parent = filesystem.open_directory_with_options(
        fixture.path(), &LocalDirectoryOpenOptions::new(),
    )?;
    publish_backup(&parent)?;
    Ok(())
})();
#[cfg(target_os = "freebsd")]
{
    let error = result.expect_err("FreeBSD 无法提供所请求的 rename 保证");
    let cause = error.downcast_ref::<qubit_local_files::error::LocalFileError>()
        .expect("发布失败会返回结构化文件系统错误");
    assert_eq!(cause.kind(), qubit_local_files::error::LocalFileErrorKind::RequirementNotMet);
}
#[cfg(not(target_os = "freebsd"))]
result.unwrap();
}
```

示例中的 `outcome.file_synced()` 只表示 `publish_new` 本身执行的文件同步，不会记录之前的 `sync_tree` 调用。默认发布选项的 `file_sync` 为 `NotRequired`，因此示例不对该字段作出断言。Required rename 耐久性取决于平台和文件系统能力；例如 Windows 会在修改 stage 权限前拒绝这一静态要求。命名空间耐久性表示操作系统同步协议的结果，不代表存储硬件已物理持久化数据。

下面的目录备份示例会在发布前保存随机名称和 identity。若目标已存在，错误会把 stage 交还给
调用方；示例先显式保留，再通过同一个 parent 按身份重新打开，确认后才清理：

```rust,no_run
use std::ffi::OsStr;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::options::{
    LocalDirectoryOpenOptions, LocalTempDirectoryOptions, LocalTreePublishOptions,
    LocalTreeValidationOptions,
};

fn create_backup(parent: &LocalDirectoryHandle) -> Result<(), Box<dyn std::error::Error>> {
    let mut stage = parent.create_staged_directory(&LocalTempDirectoryOptions::new())?;
    let mut manifest = &b"backup version 1\n"[..];
    stage.write_file_new(Path::new("manifest.txt"), &mut manifest, 1024)?;
    let name = stage.relative_name().to_os_string();
    let identity = stage.identity().cloned().ok_or("identity 不可用")?;

    match stage.publish_new(parent, OsStr::new("backup-current"), &LocalTreePublishOptions::new()) {
        Ok(outcome) => {
            assert_eq!(outcome.publication(), qubit_local_files::outcome::LocalTreePublication::Published);
        }
        Err(error) => {
            let publication = error.publication();
            let (cause, stage, retained) = error.into_parts();
            if publication != qubit_local_files::outcome::LocalTreePublication::NotPublished {
                if let Some(stage) = stage {
                    drop(stage.retain_in_place());
                }
                for resource in retained {
                    drop(resource.retain_in_place());
                }
                return Err(cause.into());
            }
            if let Some(stage) = stage {
                let location = stage.retain_in_place();
                assert_eq!(location.identity(), Some(&identity));
                let reopened = parent.open_staged_directory(
                    Path::new(&name), &identity, &LocalTreeValidationOptions::new(),
                )?;
                reopened.cleanup()?;
            }
            for resource in retained {
                resource.cleanup()?;
            }
            return Err(cause.into());
        }
    }
    Ok(())
}

let filesystem = LocalFileSystem::host()?;
let parent = filesystem.open_directory_with_options(
    Path::new("backup-root"), &LocalDirectoryOpenOptions::new(),
)?;
create_backup(&parent)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

示例为一次性备份任务选择了“失败后校验身份并清理”的处理方式。如果应用需要留待运维人员恢复，
应持久化命名空间位置与 identity，保留足够的 owner 上下文以便调用
`open_staged_directory`，并确保交接完成前不会丢弃恢复 owner。

树恢复 owner 会在 Drop 时尽力进行身份校验清理；digest 条件写入的 `LocalRecoveryFile` 则由调用方
自行恢复，不会自动删除。这两者的所有权合同不同。Windows Rooted `Content` 以及 Host `Content`
配合 `UseStaging` 会在打开 writer 时返回 `Unsupported`，早于路径解析、父目录操作或 staging 创建。
Host `Content` 配合 `PreserveExisting` 和有效 `Absent` 组合仍可用。

## 字节流协议与标准 I/O

`LocalFileReader` 使用 `ByteInput`，并保留 `std::io::Seek`。
`LocalFileWriter`、`LocalTempFile` 和内部原子 writer 使用 `ByteOutput`。这两个标记
trait 会由 `Input<Item = u8>` 和 `Output<Item = u8>` 自动实现。
这些对象使用 `Input::read_exactly` 和 `Output::write_fully`；标准 reader 可通过
`into_std_read()` 适配。

若标准 producer 写入后还要显式发布 writer，可使用借用适配器，并在提交前结束借用：

```rust,no_run
use qubit_io::{Output, OutputRef};
use qubit_local_files::LocalFileSystem;

let filesystem = LocalFileSystem::host()?;
let mut writer = filesystem.open_writer(std::path::Path::new("report.txt"))?;
{
    let mut standard = OutputRef::new(&mut writer).into_std_write();
    std::io::Write::write_all(&mut standard, b"report")?;
    std::io::Write::flush(&mut standard)?;
}
writer.commit()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

拥有型 `StdWrite<LocalFileWriter>` 可通过 `into_inner().commit()` 消耗包装器并取回 writer。
适配器转换不会 flush 或 commit。`flush` 不会发布目标，也不提供 durable 同步。原生向量 I/O
仍通过固有 `read_vectored` 和 `write_vectored` 方法提供。

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
qubit-local-files = "0.5.0"
qubit-io = "0.19"
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

`open_reader_with_options` 和 `read_prefix_with_options` 使用传入的
`LocalReadOptions` 完整替代 `defaults.read`。组合操作 `open_observed_reader`、
`read_all_with_options` 和 `read_text_with_options` 接受可选的嵌套读取选项：存在时完整替代
`defaults.read`，缺少时继承它。Snapshot builder 自行创建 `LocalReadOptions::new()`，不继承
`defaults.read`。其它操作各自的 `*_with_options` 方法仍以传入 Options 完整替代相应实例默认值。
这些规则都不在不同 Options 值之间逐字段合并。若要保留默认值中的其它字段，应先显式复制相关默认值、
修改副本，再传入。

链接策略按以下顺序确定：外层操作显式指定的策略优先，其次是所选嵌套读取选项的
`symlink_policy`，最后才是 filesystem 实例策略。Rooted 会在 I/O 前拒绝
`FollowAcrossScope`。目录子 handle 的拒绝组件名称与父 handle 的集合取并集；子 handle
不能解除父级限制，包括通过 `.` 打开时。筛选只作用于绑定输入和解析路径时遇到的组件；递归列表和复制仍可能发现并复制被拒绝名称。该策略不隔离私有子树，也不防护并发重命名。

例如，若 `defaults.read` 设置了 `open_retry_timeout`，`read_all_with_options` 未提供嵌套读取选项时
会继承该 timeout。调用 `LocalReadAllOptions::new().with_read_options(LocalReadOptions::new())` 则会
完整替代读取默认值并清除该 timeout；`LocalReadAllOptions` 的其它字段仍然生效。

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
use qubit_io::{Output, Streams};
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
writer.write_fully(br#"{"complete":true}"#)?;
let result = writer.commit()?;
assert_eq!(result.state(), LocalWriterState::Committed);
let mut bytes = Vec::new();
Streams::copy_input_to_output(&mut filesystem.open_reader(&path)?, &mut bytes)?;
let text = String::from_utf8(bytes)?;
assert_eq!(text, r#"{"complete":true}"#);
# Ok::<(), Box<dyn std::error::Error>>(())
```

多次系统调用使用的相对路径会在操作开始时绑定。复制和重命名会用同一个当前目录快照绑定
源和目标。`metadata` 观察最终条目本身，不跟随最终符号链接。

## 条件写入

能力查询只是预检提示，不保证底层文件系统一定接受原生操作。条件写入支持范围如下：

| 平台 | Host | Rooted |
| --- | --- | --- |
| Linux / macOS | `Absent`、`Content` | `Absent`、`Content` |
| Windows | `Absent`；`Content` 需使用 `PreserveExisting` 元数据策略 | `Absent`；`Content` 不支持 |
| Android | 仅编译检查；未声明协议支持 | 仅编译检查；未声明协议支持 |
| FreeBSD 14.3 | 暂不支持 | 暂不支持 |
跨平台都会拒绝 `Append`、`Content` 配合 `CreateNew`，以及跟随符号链接策略等无效条件写入配置。
Windows 不支持 Rooted `Content` 和 Host `Content` 配合 `UseStaging`。Host 与 Rooted
使用 `Required` writer durability 时，都会在路径解析或创建 staging 前返回
`RequirementNotMet`。Linux 和 macOS 可尝试有效配置下的 `Required` 耐久性与 `UseStaging`，
但仍受所选文件系统和实际运行操作约束。应用需要展示能力提示时可查询
`LocalDirectoryHandle::conditional_support`，但仍须处理打开和提交错误。Android 仍配置为编译检查；
FreeBSD 暂不支持。其 CI 只检查编译和少量兼容性合同，不再运行完整行为测试套件。各 API 的失败用例和观察到的原因见[FreeBSD 兼容性说明](freebsd-compatibility.zh_CN.md)。

当应用读取文档 A、基于它准备新内容 B，并且不能覆盖其他人后来保存的版本时，使用
条件写入。Content 成功与竞态测试仅在 Linux 和 macOS 启用，符合当前实现的平台范围；其他 Unix 的 Content
操作保留 `Unsupported` 合同，`Absent` 有独立合同。应用清单需启用可选 feature，并加入算法实现：

```toml
# BEGIN executable application dependencies
[dependencies]
qubit-local-files = { version = "0.5.0", features = ["digest"] }
qubit-io = "0.19"
sha2 = "0.11"
tempfile = "^3.27"
# END executable application dependencies
```

下面的完整示例先创建 A，计算 SHA-256 摘要，再尝试以 B 替换。随后演示冲突分支：
在第二次提交前，另一个写入者把 A 改成 C；提交因此失败，应用从保留的 candidate
副本中读回 B。
这个 feature 由 `qubit-local-files` 依赖开启；应用本身无需声明 `digest` feature。

```rust
# #[cfg(not(feature = "digest"))] fn main() {}
# #[cfg(feature = "digest")] fn main() -> Result<(), Box<dyn std::error::Error>> {
// BEGIN executable conditional write example
use qubit_io::{Output, Streams};
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::{
    LocalWriteMode, LocalWriteOptions, LocalWritePrecondition,
};
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::{Digest, Sha256};

let directory = tempfile::tempdir()?;
// Reject 同样拒绝系统临时目录路径中的链接，先解析自有临时目录。
let path = directory.path().canonicalize()?.join("document.txt");
std::fs::write(&path, b"A")?;
let mut filesystem = LocalFileSystem::host()?;
filesystem.set_symlink_policy(LocalSymlinkPolicy::Reject)?;
let old_digest = Sha256::digest(std::fs::read(&path)?);
let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
let mut writer = filesystem.open_conditional_writer::<Sha256>(
    &path, LocalWritePrecondition::Content(old_digest), &options,
)?;
writer.write_fully(b"B")?;
let saved = writer.commit()?;
assert_eq!(saved.digest(), &Sha256::digest(b"B"));
assert!(saved.cleanup_errors().is_empty());
assert!(saved.retained_files().is_empty());

// 第二次编辑仍从 A 开始；提交前，另一个写入者把它改成 C。
std::fs::write(&path, b"A")?;
let expected_a = Sha256::digest(b"A");
let mut conflicted = filesystem.open_conditional_writer::<Sha256>(
    &path, LocalWritePrecondition::Content(expected_a), &options,
)?;
conflicted.write_fully(b"B")?;
std::fs::write(&path, b"C")?;
let failure = conflicted.commit().expect_err("C 与 A 不再匹配");
assert_eq!(failure.reason(), qubit_local_files::error::LocalConditionalFailureReason::PreconditionMismatch);
assert_eq!(std::fs::read(&path)?, b"C");
let candidate = failure.candidate().expect("B 的恢复副本仍被保留");
let mut reader = candidate.open_reader()?;
let mut recovered = Vec::new();
Streams::copy_input_to_output(&mut reader, &mut recovered)?;
assert_eq!(recovered, b"B");
// END executable conditional write example
# Ok(())
# }
```

`LocalWritePrecondition::Absent` 用于仅创建、不覆盖的发布；只有后端支持安全的
no-overwrite 安装时才可用。`Content(digest)` 必须搭配 `CreateOrReplace`，不能搭配
`Append`。`LocalWriteOptions` 还控制原子性与耐久性。对于 writer，`Required` 要求同步文件数据及相关命名空间更新；对于 rename，它要求同步命名空间更新。后端无法满足所选操作的要求时会返回 `Err`。耐久性设为 `Preferred` 时，若同步不可用，结果会报告较弱的
耐久性；原子性要求不会降级为普通覆盖。

默认 `NotRequired` 会跳过清理阶段的命名空间同步。`Preferred` 和 `Required` 仍会在删除
临时内容及 candidate 副本后尝试同步；清理同步失败会通过成功结果的 `cleanup_errors()` 报告，
不会撤销已发布文件，也不会改写其耐久性结果。

成功表示发布及必需的同步已完成。之后库会清理临时内容和独立 candidate 副本；若
清理失败，成功 outcome 仍会通过 `cleanup_errors()`、`retained_files()` 告知调用方。
失败时检查 `reason()`、`stage()`、`publication()`；保留错误对象，或用
`into_recovery_files()` 接管恢复文件。如果需要找回 B，应在错误对象销毁前调用
`candidate().open_reader()`。恢复句柄只读，销毁句柄不会删除文件。尚未提交时调用
`abort()` 或直接丢弃 writer 会取消发布并尝试清理；`commit()` 出错则把可恢复状态
交给错误对象。

该协议不提供严格的 compare-and-swap。发布前会检查旧内容，发布后会检查被替换内容；
但其他进程仍可能通过已打开的旧 inode 修改文件，ABA 也可能让变化后的内容再次具有相同
摘要。staging 与恢复文件名可能在父目录中可见。回滚至多尝试一次。进程被 abort 或
机器崩溃可能中断发布或清理；库不会在重启时扫描遗留文件。平台范围方面，Windows
Host 的 Content 仅支持 `PreserveExisting` 元数据策略；Windows Rooted Content 不受支持。
Rooted Content 与 Host Content + `UseStaging` 都在打开 writer 时拒绝，不会解析路径或创建 staging。
Rooted 操作使用已打开的目录句柄。这里不宣称 Android 或 FreeBSD 支持 Content。
Windows 上，使用 `Required` 耐久性打开条件 writer 会在路径解析或创建后端资源前返回
`RequirementNotMet`。恢复文件和条件提交错误满足 `Send + Sync`；条件写入 outcome 也会在
其 digest 类型满足相应约束时满足 `Send + Sync`。这不表示打开中的 writer 可跨线程安全使用。

Windows 会用受保护的当前用户 DACL 创建条件候选文件及其恢复副本。因此，`Absent` 条件写入
新建的目标可能比父目录继承的权限更严格；`PreserveExisting` 仍保留已有目标的保护方式。
应用若要共享新建文件，应在成功发布后通过自己的显式 ACL 流程授权，不要为放宽候选文件权限
而提前削弱 staging。该平台行为需要原生 Windows 运行证据确认。

Conditional writer 创建时会保存本次操作的 PWD context。之后的 commit、abort 和 cleanup 错误
使用该快照，不会重新查询可能已经变化的进程 PWD；Host 物理路径只是可选诊断，也不会替换
Rooted namespace path。

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
Windows Rooted writer 若在安装前最后一次 identity 检查中发现目标已被替换，会返回
`NotPublished`，保留 staging 并保持竞争目标不变。native rename 已经尝试后的错误应按类型化
commit failure 中记录的发布状态处理，不能套用这个结论。

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

目录树复制可能先删除与源类型冲突的目标，再开始复制替代内容。后续步骤失败时，应同时检查
`LocalCopyFailure::state()` 与 `partial_stats()`；即使尚未计入已复制文件，
`PartiallyPublished` 也会保留目标已被删除的事实。不能只凭 `Err` 推断目标未改变。

使用 `with_create_parent()` 时，复制会先创建缺失的目标父目录，再打开源并发布目标内容。
如果后续步骤失败，这些辅助目录会保留；它们不代表目标内容已经发布。因此，只要请求的目标
本身没有改变，`LocalCopyFailureState::Unchanged` 仍然准确。库不会自动删除这些目录，因为其他
并发操作可能已经开始使用它们。

### 复制源模式

Host 与 Rooted 中，`files()` 包括复制的链接，`directories()` 只计入新建目录，
`bytes()` 只计入普通文件字节。`overwritten()` 包括替换的条目和 `Overwrite` 策略下
合并的已有目录，包括复制根目录；`Skip` 策略下的目录合并不计为覆盖。

一次复制若成功跳过所有条目，就不会发布目标，也不会执行发布后的父目录同步；结果为 `atomic = false`、
`durable = false`、`metadata_preservation = None`。实际复制的零字节文件仍会计入 `files` 并按正常流程发布。
失败时即使文件统计为零，也要检查类型化失败状态和部分统计，不能据此认定没有副作用。

| 模式 | 普通文件 | 最终链接，包括悬空链接 | 实体目录 | 特殊文件 |
| --- | --- | --- | --- | --- |
| `Entry` | 复制内容 | 复制链接本身 | `RequirementNotMet` | `Unsupported` |
| `Tree` | `RequirementNotMet` | `RequirementNotMet` | 复制目录树 | `Unsupported` |
| `Auto` | 按条目复制 | 按条目复制 | 按目录树复制 | `Unsupported` |

Copy Options 默认使用 `LocalMetadataPreservePolicy::None`。显式请求 `Permissions` 后，
文件复制会先把源权限应用到 staging，再同步并发布。发布前失败时已有目标仍保留；若此前已
删除冲突目标，类型化失败会反映这项部分变更。该选项只复制权限元数据，不复制源 ACL 或所有文件元数据。

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

Rooted 文件复制使用 `Replace` 覆盖不兼容的最终目标（例如已有目录）时，会先把完整替代文件复制到
staging，再删除冲突目标。打开源、复制内容或预算耗尽等删除前失败会保留原目标及其内容。
删除前还会复核目标类型和 identity；并发替换出的新条目会保留。删除后再失败仍可能是
`PartiallyPublished`，整个过程不提供完整事务回滚。

## 遍历和临时资源

`LocalFileSystem::list` 返回惰性的 `LocalDirectoryWalker`。它按需打开和推进目录；
绑定后的命名空间 root、Options、符号链接策略、PWD snapshot 和 authority 在创建时固定。默认不设置
深度、条目数、名称内存、deadline 或打开目录数预算。调用方设置打开目录预算后，
`Reopen` 会按需关闭并重新打开活动 frame，`Fail` 则会在边界返回 `ResourceLimit`。
零句柄预算无效并返回 `InvalidOptions`。Rooted 会逐项读取目录，避免先收集到 `Vec`；
drop walker 只释放句柄。Host 列举即使跟随符号链接进入另一个物理目录，也会保留请求的
namespace 路径作为公开 root；可选的 diagnostic path 仍可记录实际访问的物理路径。
`max_depth` 为零时结果为空，并且不读取目录子项；深度、条目数和 seen-name 字节预算都为零时，
同样不会读取子项。Host walker 在跳过已见名称期间及下一次目录读取前检查 deadline。Rooted 列表也会在
路径与符号链接解析期间（包括每次展开）检查期限，并复用已解析的起始目录。这些期限都是协作式的，
不能中断已经阻塞的原生操作。未配置期限时，Rooted 解析不会施加隐藏的符号链接展开上限。
只有配置 seen-name 字节预算，或有限打开目录预算下的 `Reopen` 可能重新访问 frame 时，walker 才保留
已返回名称。无缓存时，并发目录变化遵循原生 reader 语义，不提供快照或全局 exactly-once 保证；
重开的 frame 仍会抑制先前已返回的名称。
不允许向下遍历时，最终链接按自身 metadata 返回，不会先解析目标；Host 非递归列表因此会
保留悬空链接条目。递归跟随目录链接时，悬空或越界链接会产生错误。`Continue` 会返回局部子目录
打开错误并继续安全的同层条目；全局预算或 deadline 耗尽仍会终止遍历。

临时文件和目录按当前源资格承担清理责任。每个资源都创建在独立的私有 sandbox 中，
sandbox 会和资源一起清理。需要观察清理失败时应显式调用 `cleanup()`；drop 只会静默地尽力清理。
`keep` 会原子发布到 sandbox 外生成的 sibling 路径，返回 `LocalPersistOutcome`，其 cleanup state
会报告 sandbox 残留。未显式指定 parent 时，在该次操作捕获的 filesystem PWD 下创建。
`path()`、`keep` 和持久化结果对 Host 与 Rooted 都返回 namespace-absolute 路径，因此
后续 PWD 即使变化，也能把它们再次传给同一个 filesystem。持久化失败会保留资源，后续允许
的操作由发布状态和源资格共同决定。创建前会组合前缀、随机名称和后缀并校验完整组件；原生
分隔符、NUL、控制字符、Windows 禁用标点、末尾空格或句点，以及 `CON.txt` 这类设备保留名称
都会被拒绝，且校验先于父目录创建。名称冲突时，`max_attempts` 为 `None` 表示不限尝试次数；零表示不尝试。非冲突原生错误立即返回。

`LocalDirectoryHandle::create_staged_directory` 始终在该 handle 的固定 authority 下直接创建。
`LocalTempDirectoryOptions` 中的 `prefix()`、`suffix()` 和 `max_attempts()` 会用于 stage 名称及
重试预算；`parent`、`create_parent` 不会替这个入口选择或创建其他父目录。stage 的深度上限用于
验证整棵树：位于上限的条目有效，超出上限返回 `ResourceLimit`，空树在深度零也有效。
展示用 walker 仍会按 `max_depth` 截断输出。

`LocalTempFile::creation_parent()` 与 `LocalTempDirectory::creation_parent()` 返回创建时捕获的
绝对父目录坐标。Host 返回 backend 实际使用的父目录（可能已解析调用方提供的别名）；Rooted
返回虚拟根下的绝对路径，不暴露 `diagnostic_root()`。该快照在 close 或 cleanup 后仍可查询。
适配器需要把资源路径映射到调用方请求的 parent 时，可相对 `creation_parent()` 计算路径后缀，
再保留请求中的别名；不要从 sandbox 名称或固定目录层级反推父目录。

### 让读取结果仍对应原名称

如果业务决定必须针对刚读取的那个名称对象，读取后可以检查路径绑定。下面用 Rooted authority
打开配置文件、读取字节，再检查名称仍否指向原对象：

```rust
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::observation::LocalSourceVerification;
use qubit_local_files::options::{LocalObservedReadOptions, LocalPathBindingOptions};

let fixture = tempfile::tempdir()?;
std::fs::write(fixture.path().join("settings.toml"), b"mode = 'safe'\n")?;
let filesystem = LocalFileSystem::rooted(fixture.path())?;
let binding = LocalPathBindingOptions::new()
    .with_max_ancestors(32)
    .with_max_path_bytes(4096)
    .with_deadline(Duration::from_secs(1));
let options = LocalObservedReadOptions::new().with_path_binding(binding);
let mut reader = filesystem.open_observed_reader(Path::new("/settings.toml"), &options)?;
let bytes = reader.read_to_end_limited(4096)?;
match reader.verify_path_binding()? {
    LocalSourceVerification::Unchanged => {
        // 本次请求可以按 `bytes` 作出决定。
        let _accepted_bytes = bytes;
    }
    LocalSourceVerification::Changed | LocalSourceVerification::Missing => {
        // 丢弃这次决定，重新打开名称并读取当前内容。
        return Ok(());
    }
    _ => return Ok(()),
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Changed` 和 `Missing` 是普通验证结果，不是错误。复核会在保留的 authority 和解析合同下重新解析原名称。
路径绑定的解析限制会与外层 filesystem 或 directory handle 的限制取较严格值，因此不能放宽外层限制。
检查通过后并不会锁住路径；对象 identity 也不等于文件内容版本。若业务约束的是内容，需比较内容或使用快照。
未启用绑定时调用 `verify_path_binding()` 会返回 `InvalidState`；Windows Host 无法提供绑定，会返回
`Unsupported`。祖先数、路径字节数、解析预算和期限限制绑定过程的资源使用；期限是协作式的，不能中断阻塞的原生调用。

## 临时资源发布与失败恢复

临时文件写完后才能发布。`persist` 和 `persist_with` 消耗 guard，接收命名空间绝对目标；
相对目标通过 `persist_at` 明确指定基准目录。`LocalPersistOptions::new()` 遇到已有目标会失败。
成功时返回 `LocalPersistOutcome`，调用方应检查路径、原子性、耐久性、`cleanup_state()` 和
`cleanup_error()`。sandbox 残留通过成功结果报告，不会撤销发布，也不表示可以再次发布。
文件同步前会核对当前打开句柄是否仍指向 guard 持有的源实体。通过 `as_file_mut()` 换入其他
文件句柄不会得到针对该文件的耐久成功；若内容句柄已关闭，库会经创建时的 authority 重新打开，
并在同步前再次核对实体身份。Linux 普通用户下已验证私有句柄同步 I/O 的 `NotRequired`、`Preferred` 和 `Required` 路径；Windows 原生运行行为仍待验证。

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

### 处理 seal 失败

`seal_with_options` 会消费可写 owner。失败后先查看 `cause()` 和 `access_state()`，再用
`into_resource()` 接管临时资源。状态可帮助应用判断是否仍可考虑有限重试；本例在所有状态下都选择显式
cleanup。若 cleanup 失败，返回的应用错误会同时持有 cleanup 产生的 `LocalFileError` 和
`LocalTempFile`。`LocalFileError` 自身不会重新获得已经取出的 owner。所有权和清理合同见
[`LocalTempRecovery` Rustdoc 源码](../src/temp/local_temp_recovery.rs)。

```rust
use qubit_io::Output;
use qubit_local_files::{LocalFileSystem, LocalSealOptions};

// BEGIN executable seal cleanup helper
use qubit_local_files::error::LocalTempSealError;
use qubit_local_files::{LocalFileError, LocalTempFile};

#[derive(Debug)]
struct PendingSealCleanup {
    cause: LocalFileError,
    resource: LocalTempFile,
}

impl std::fmt::Display for PendingSealCleanup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "temporary cleanup remains pending: {}", self.cause)
    }
}

impl std::error::Error for PendingSealCleanup {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

fn cleanup_failed_seal(error: LocalTempSealError) -> Result<(), Box<PendingSealCleanup>> {
    let cause = error.cause().to_string();
    let access_state = error.access_state();
    eprintln!("seal 失败：{cause}");
    eprintln!("seal 访问状态：{access_state:?}；本例选择清理");
    let mut resource = error.into_resource();
    match resource.cleanup() {
        Ok(()) => Ok(()),
        Err(cause) => Err(Box::new(PendingSealCleanup { cause, resource })),
    }
}

fn retry_pending_seal_cleanup(
    retained_error: Box<dyn std::error::Error>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut pending = retained_error
        .downcast::<PendingSealCleanup>()
        .map_err(|error| error)?;
    match pending.resource.cleanup() {
        Ok(()) => Ok(()),
        Err(cause) => {
            pending.cause = cause;
            Err(pending as Box<dyn std::error::Error>)
        }
    }
}
// END executable seal cleanup helper

let filesystem = LocalFileSystem::host()?;
let mut temporary = filesystem.create_temp_file()?;
temporary.write_fully(b"generated data")?;
match temporary.seal_with_options(&LocalSealOptions::new()) {
    Ok(mut sealed) => sealed.cleanup()?,
    Err(error) => {
        cleanup_failed_seal(error)
            .map_err(|pending| pending as Box<dyn std::error::Error>)?;
        return Err(std::io::Error::other("seal 失败，已清理恢复资源").into());
    }
}
# Ok::<(), Box<dyn std::error::Error>>(())
```

seal 成功后，原 writer owner 已被消费，不能再用它写入；返回的 owner 只允许读取和清理。
若 cleanup 失败，资源由 `PendingSealCleanup` 持有，而非 `LocalFileError`。应用应返回或保留这个错误，之后再按
cause 和资源当前状态决定是否重试。选择重试时，先把应用错误向下转型取回资源，再调用示例中与 helper 并列的
`retry_pending_seal_cleanup` 函数。
调用时将保留的错误作为 `Box<dyn Error>` 传入。如果再次 cleanup 仍失败，函数会更新 cause，并返回仍持有
资源的同一个应用错误。再次决定是否重试前，应检查这次返回的错误和资源状态。

如果暂不适合重试，就继续保留错误；只保存路径字符串无法保留资源所有权。丢弃错误也会丢弃 owner，并触发尽力清理。
不要无条件循环重试，也不要在原路径直接创建替代条目。

### 从输出名称冲突中恢复

下面在独立临时 parent 中暂存清单，利用 no-replace 冲突稳定地产生失败，再显式检查清理结果。
已有输出的内容保持不变。需要重试时，先确认保留资源当前仍为 `Owned`。

```rust
use qubit_io::Output;

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
qubit_io::Output::write_fully(&mut temporary, br#"{"complete":true}"#)?;

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

条件写入清理已删除旧文件、但无法移除其恢复容器时，错误路径指向容器；已删除文件不会作为保留资源返回。
容器中的意外条目会保留，调用方应通过单独授权的文件系统操作处理。

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
接收者；对于这些 list/copy/delete 操作，`*_with_options` 仍完整使用传入 options。

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
use qubit_io::Output;
use std::path::Path;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;

let filesystem = LocalFileSystem::host()?;
let base = std::fs::canonicalize(std::env::temp_dir())?;
let mut temporary = filesystem.create_temp_file()?;
qubit_io::Output::write_fully(&mut temporary, b"generated report")?;
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

错误对同时保留主 I/O 错误与清理错误。默认 Host 临时文件或目录创建若在 owner 接管前失败，且清理残留也失败，
`LocalFileError` 会分别保留主错误、`cleanup_error()` 和 `temp_recovery()`。应用应保留该错误，提取恢复 owner，
在清理条件允许后显式重试；不要只按错误文本或路径删除。创建失败发生在检查之前，保存在结构化错误位置。
正式应用可用具名错误类型包装这两个独立信息，避免清理错误覆盖主错误。

```rust
use std::io;
use qubit_io::Output;
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
    let staged = qubit_io::Output::write_fully(&mut file, payload);
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

实例默认 Options 是便利配置，不是强制上限。直接操作的 `*_with_options` 值会替换对应默认值；
组合读取遵循“配置一次，显式覆盖”一节说明的嵌套选项继承规则。需要请求无法放宽的 provider 上限时，
应配置 `qubit-fs-local::LocalResourcePolicy`。

删除操作有明确的类型契约：`delete_file` 遇到实体目录返回 `IsDirectory`，
`delete_directory` 遇到普通文件或最终符号链接返回 `NotDirectory`。`missing_ok` 只对请求的
根条目不存在时生效。递归删除已移除条目后失败时，`LocalFileError` 会保留
`PublicationIncomplete`；重试前应分别检查 `effect_state()` 与 `cause_kind()`。基础错误的
`effect_state()` 返回 `None` 表示没有足够证据推断副作用，不能当作 `Unchanged`。


## 有界解析与安全策略

打开 Rooted authority 时，如果需要限制路径解析工作量或检查应用自定义的名称规则，可配置
`LocalRootOpenOptions`。四类限制分别控制符号链接跳转次数、原始链接目标字节数、实际路径组件数和
解析路径字节数。`None` 表示调用方不增加限制，`0` 是实际的零上限。Unix 按 native bytes
计长；Windows 按 UTF-16 code unit 乘 2 计长。等待处理的路径与已解析目录前缀分别计量。
子目录选项继承父级合同，只能收紧限制；传入 `None` 不会取消父级的有限上限。
`LocalResolutionLimits` 实现 `Copy`，其 builder 是 `const`；root open options 是拥有配置值。
默认配置没有新增额度。

`LocalObservedReadOptions::with_resolution_limits` 可为 Rooted observed read 设置有限解析预算。
如果 Host observed read 的有效预算有限，包括由 path binding 提供的预算，则返回 `Unsupported`；
Host 请求未设置有限预算时保持原行为。需要对 observed read 强制执行有限解析预算时，应使用 Rooted。
预算作用于该接口实际解析的路径；普通 Rooted 操作在词法规范化中消去的组件不会被访问或计费。

`LocalResolutionPolicy` 提供纯校验回调，检查普通路径组件和原始符号链接目标。回调能区分正在打开
Rooted、还是从既有 authority 向下解析，也能区分名称来自调用方输入还是链接目标。对应 metadata
查询或 open 之前会调用校验；回调不得执行 I/O，也不能依赖固定调用次数。既有组件 deny-name
策略仍单独生效，并与它组合。该回调约束 resolver 实际访问的组件，不会自动成为递归遍历的全树拒绝清单：walker 或 tree copy 发现一个子项名称，本身不保证对该名称调用此回调。若要检查完整暂存树，应使用 expected entries 或显式拒绝规则进行树验证。

不同接口的路径语义不同：

| 输入 | 解析行为 |
| --- | --- |
| 普通 Rooted 操作，例如 `rooted.open_reader(Path::new("missing/../file"))` | 先对调用方路径做词法规范化，因此可能不访问 `missing`，直接打开 `file`。 |
| 传给 `LocalFileSystem::rooted_with_options` 的 Host 原生路径 | 按组件顺序遍历；`missing/../dir` 必须先访问 `missing`，缺少时失败。Root-open 预算计量这条原生构造路径。 |
| 符号链接原始目标或固定目录 handle 解析的路径 | 遵循对应解析器逐组件解析的合同；不能套用普通 Rooted 调用方路径折叠规则。 |

Rooted 有限解析预算按相应解析层实际处理的工作计量；适用时包括链接目标组件和已解析前缀。
字面 `.`、`..` 会先计入预算，再按解析器语义处理。有限额度在展开或分配对应工作前检查；超限通过
`LocalFileErrorKind::ResourceLimit` 报告具体维度。

安全观测选项区分 `Optional` 和 `Required`。只有机制明确不支持时，Optional 才会省略相应字段；权限拒绝、
普通 I/O 错误和损坏 ACL 仍返回错误。Required 在无法取得完整事实时失败。ACL 条目数和字节数默认
为 `None`，设为 0 则不允许对应数据。查询直接使用已保留的文件或目录对象，不按诊断路径重新打开。
结果由一系列观察组成，不是 ACL 与 identity 的原子快照，也不保证返回后权限保持不变。

Linux/macOS 以外的 Unix 目标尚未实现文件系统事实查询：请求 `Optional` 时相应字段为 `None`，请求 `Required`
时返回 `RequirementNotMet`。identity、link count 等其他事实仍可单独查询。这是明确的不支持结果，不会吞掉真实 I/O 错误。

`LocalOwnerAccessPolicy` 可精确设置 Unix 目录/文件 mode，或 Windows 目录/文件 access mask。
Unix 使用 `with_unix_modes(directory_mode, file_mode)`，Windows 使用
`with_windows_masks(directory_mask, file_mask)`，只填写目标平台的设置；策略没有隐含权限预设。
非空 staged tree 若使用该策略，库必须能读取/列出目录并遍历目录，才能完成发布前的最终重走检查。
缺少这些权限时会在 mutation 前拒绝发布。只有在策略已应用并验证后，最终 access state 才会报告
`OwnerPolicyApplied`。未设置 `LocalListOptions::max_entries` 时，修改权限前最多固定 256 个子项 identity
和管理句柄；树更大时会在 mutation 前以 `ResourceLimit` 失败。显式设置 `max_entries` 会替换该默认值，
可以调高或调低；调高可能同时保留更多 identity 和管理句柄。这不代表不同平台的 ACL 继承或有效访问语义完全相同。

请求文件同步时，权限策略应用并验证后，库还会通过保留的句柄再次同步普通文件。
只有所有请求的文件同步成功，`file_synced()` 才为 true。`NotRequired` 不增加权限变更后的文件同步；
`Preferred` 在最终同步失败时仍可发布，但 `file_synced()` 为 false；`Required` 会阻止发布并保留 stage。
访问状态反映已经发生的权限变更。

macOS 上尚未实现精确 Unix owner-access ACL 验证，因此需要该验证的 owner policy 会返回
`Unsupported`。单独设置 mode bits 不表示扩展 ACL 已排除其他身份；库不声称在未经验证的 Darwin ACL
上实现了 `CurrentUserOnly`。

Windows 的访问转换尚无原生运行验证。实现设置 `read_only=false` 时只恢复 DACL，不会清除已有的
`FILE_ATTRIBUTE_READONLY`；因此恢复当前用户 ACL 不保证一个原先带只读属性的文件可重新写入。

Windows Rooted 条件 `Content` 发布仍为 `Unsupported`。Windows 尚未证明所需的相对目录发布和恢复协议，
不能把 Unix 支持矩阵当成 Windows 原生证据。

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
FreeBSD 暂不支持；原生 workflow 只做编译和少量合同检查，不运行完整行为套件。详细失败 API 和用例见[兼容性说明](freebsd-compatibility.zh_CN.md)。
Android 仍配置为编译检查。T3/T4 的 Windows 清理与条件写入行为，以及 LF-09 私有资源和
LF-10 sealed 清理，尚无原生 Windows runner 证据时必须视为待验证：Linux 对应筛选为 0 tests，Windows
GNU `cargo check --lib` 只能证明可编译，不能证明运行行为。
本 crate CI 仅验证本 crate；下游 adapter 的兼容性和集成由 adapter 仓库或独立集成负责。
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

依赖版本和校验和由 `Cargo.lock` 固定。可用 `cargo test --locked` 验证当前 crate；GitHub Actions 还会按 CI 配置检查当前 crate 的 feature 组合与平台。adapter 的兼容性和集成由 adapter 仓库或独立集成 job 验证。性能测量应与正确性测试分开执行：

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
FreeBSD 当前支持边界见 [FreeBSD 兼容性说明](freebsd-compatibility.zh_CN.md)。
## 观测要求与累计计数

普通 native observation 的 Optional 可省略不可用的 native identity 或时间扩展；基础 metadata 查询失败仍会传播。Required 要求当前入口请求的完整事实：Unix 查询失败保留 I/O 原因；Windows 查询失败归类为 `RequirementNotMet` 并保留 source。Security observation 使用独立合同：Optional 仅可省略明确不支持的事实，权限、I/O、格式和身份错误仍传播；Required 将请求事实不完整或不支持报告为 `RequirementNotMet`。

固定目录句柄按调用方原始相对路径逐组件解析，并在解析期间应用句柄与调用预算，不会先将路径词法归一化后替代原始解析配方。`LocalWriteOutcome::bytes_written()` 现返回 `u64`，单次 I/O 调用和切片长度仍为 `usize`。将累计值绑定到 `usize` 的调用方应迁移到 `u64`；只有需要索引时才进行可检查的转换。

RLF2-Q02 已证实并修复：Unix 清理通过保留的父目录执行 no-follow metadata 身份验证，因此 owner-read 权限不是额外清理前提。RLF2-Q01 与 RLF2-Q03 未验证，缺 Windows 原生环境。
