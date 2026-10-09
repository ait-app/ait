# ADR-112：file 瘦身为 persistence，宿主专用文件归 daemon

- 状态：接受
- 日期：2026-10-09
- 修订：[ADR-105](adr-105-file-tools-and-startup-config.md) 中启动配置归 file 的决定；
  [ADR-106](adr-106-concrete-file-persistence.md) 中 server identity 归 file、metadata 在生产代码中
  依赖 file 的决定，以及 crate 名称；
  [ADR-107](adr-107-direct-imports-from-owning-crates.md) 中“仅测试依赖”规则的适用范围。

## 背景

file 同时承载三类内容：单文件读写、观察和通用 registry；实现 model/domain 存储契约的
具体文件适配器；以及只有 daemon 使用的启动 CLI/TOML 配置、故障证据文件和稳定 server
identity。第三类使 file 依赖 `clap`、`toml`、`anyhow`、`secrecy` 和 `uuid`，却没有其他消费者。
metadata 的 workspace 自动化还直接调用 `file::storage::project_config::read_path`，
是唯一一条在生产代码中从功能 crate 指向具体文件实现的边。`file` 这个名称也容易与
`filesystem` 混淆。

## 决策

1. crate 改名为 `persistence`，目录为 `crates/persistence`。它保留 `File`、`watch`、
   `registry::FileRegistry` 和 `storage::*` 文件适配器，仍只依赖 model 和 domain。
   创建回执适配器从 `file::creation` 移到 `persistence::storage::creation`，所有具体适配器
   都位于 `storage` 下。
2. 宿主专用模块迁回 `bins/daemon`：
   - `config`（CLI、环境变量与 TOML 优先级、凭据脱敏）位于 `bins/daemon/src/config.rs`；
   - 故障证据文件的保存、清理与读取位于 `bins/daemon/src/diagnostics/store.rs`；
   - 稳定 `server-id` 的加载与不覆盖发布位于 `bins/daemon/src/instance/identity.rs`。
   它们通过 `persistence::File` 原子写入；对应依赖 `clap`、`toml`、`secrecy`、`serde`、
   `tempfile` 和 `thiserror` 由 daemon 声明，persistence 移除 `anyhow`、`clap`、`toml`、
   `secrecy` 和 `uuid`。配置优先级、路径、格式和 identity 发布语义保持不变。
3. `model::storage::project::ProjectConfigStore` 增加 `config_path(root)`：返回读取将使用的
   项目配置文件路径（存在 `ait.json` 时即使无效也优先，仅在其不存在时回退 `paseo.json`）。
   `LocalProjectConfigStore` 用原有解析实现它，原 `read_path` 自由函数不再公开。
4. `LocalWorkspaceAutomation::new` 接收 `Arc<dyn ProjectConfigStore>`，脚本和 setup 配置
   通过该端口定位文件；读取上限、符号链接拒绝和错误文本不变。daemon 注入已有的
   `LocalProjectConfigStore`。metadata 对 persistence 的依赖移到 `[dev-dependencies]`。
5. 依赖守卫登记 `persistence` 并将 `file` 加入退役包名单；除 daemon 外，所有 workspace
   包只能以 dev 依赖使用 persistence。原有 persistence 向外依赖、传输与数据库依赖的拒绝规则
   随包名迁移。

## 后果与验证

只有 daemon 在生产代码中连接具体文件适配器；功能 crate 全部通过 model 端口协作。
persistence 不再携带启动配置与 CLI 依赖，daemon 拥有自身进程专用的状态文件。
Rust 导入路径从 `file::` 改为 `persistence::`，存储格式和用户数据目录不变。

验证覆盖 persistence 单元测试（含新增的 `config_path` 偏好与错误用例）、metadata 的
workspace 自动化与目录服务测试、daemon 的 config/diagnostics/instance 单元测试和依赖守卫。
