# Daemon 重复注册、旧绑定与删除主机后登录验证

日期：2026-10-05。平台：macOS aarch64。

源码基线：Ait `3b10f326`，中心 `ait-app/ait-server`
`87dc0768d1b8c8440e4c20d305261da49ba88519`，均包含本轮未提交改动。
中心改动已写回 `/Users/necokeine/Documents/ait-server`；其源码与隔离验证副本一致，
[覆盖率摘要](daemon-registration-coverage.json)包含中心变更文件的 SHA-256。

## 结果与范围

旧中心代码在真实 PostgreSQL 中复现
`POST /v1/nodes/register: 409 Conflict / Resource already exists`。
原因是客户端每次为同一 daemon 生成不同 `installation_id`，而中心永久约束
`nodes.host_id` 唯一，关闭租约不会释放节点与 Host 的绑定。

客户端改为使用 daemon 的 `server_id` 作为节点安装身份，保留每次租约独立的
`registration_id`，并在注册 ID 已关闭时允许下一次同步使用新 ID。
中心在注册事务中迁移旧绑定，保持 Host ID、旧客户端租约及禁用状态。
活动 runtime 租约继续阻止接管。详见 [ADR-085](../../decisions/clients/adr-085-stable-daemon-publication.md)。

删除或禁用旧 Host 后，纯客户端注册还会因节点残留的 Host 绑定返回
`403 Forbidden / Insufficient permission or inactive account`。回归测试中密码登录成功，
随后节点注册复现该错误；这与密码错误或账号失效不是同一路径。
中心现在在纯客户端注册事务中解除失效绑定，并撤销旧 runtime 租约，避免绑定解除后
旧会话重新获得权限。客户端登录和新租约续租恢复，Host 删除/禁用记录保持不变；
显式 runtime 注册仍拒绝重新启用该 Host，节点本身被禁用或删除时仍拒绝注册。

通用桌面 IPC 的调用与处理两端统一使用 `ait:invoke`；旧名称来自 Paseo 导入的
Electron 桥接代码。未扩展为其他 IPC、全局变量或存储键的改名。

## 测试执行

只运行直接相关测试，合计 46 项通过：共享账户 15 项、桌面 17 项、共享界面 7 项、中心 7 项。
本次删除主机后的登录补修只改变中心 Rust 行为，重新执行了中心 7 项；客户端 39 项沿用
同一未修改源码上的先前验证结果。

```sh
npm run test --workspace=@ait/client -- src/account-session.test.ts
npm run build:sdk
npm exec --workspace=@ait/desktop -- vitest run src/preload.test.ts src/preload-sandbox.test.ts src/daemon/daemon-manager.test.ts src/daemon/account-session.test.ts
npm run test --workspace=@ait/mobile -- src/runtime/online-service-host-sync.test.ts src/screens/settings/online-service-host-section.test.tsx
```

中心使用临时目录中的独立 PostgreSQL，监听 `127.0.0.1:55473`，与应用运行数据隔离。
在中心目录执行：

```sh
DATABASE_URL=postgres://ait_registration_test@127.0.0.1:55473/ait_registration cargo test --offline --locked --features integration-tests --test api relay::registration
```

中心覆盖旧绑定迁移、幂等重试、停止后重新启用、runtime 重启、旧客户端租约保留、
活动 runtime 冲突与到期恢复、禁用/删除节点保护、随机安装 ID 拒绝及回滚。
迁移、停止及重启路径还实际签发控制票据、建立控制 WebSocket 并验证 welcome。
删除/禁用主机后的回归覆盖密码登录、纯客户端注册、在线主机发现、新租约续租、旧 runtime
租约续租被拒绝，以及 runtime 重新注册不能恢复已删除或禁用的 Host。

格式、lint 和静态检查通过：变更 TypeScript 的 `oxfmt --check`、`oxlint`，
`@ait/client` 与 `@ait/desktop` 的 typecheck，中心 `cargo fmt --check`、
`cargo clippy --offline --locked --all-targets --features integration-tests -- -D warnings`，
两仓库 `git diff --check`，Ait 文档链接检查。

## Test coverage

使用 `cargo llvm-cov` 只执行中心 `api` 集成目标中 `relay::registration` 匹配的 7 项测试，
启用 `integration-tests`。34 项无关集成测试被过滤；没有运行全套测试或 workspace 覆盖率。
没有额外文件排除，使用工具默认的依赖及测试源码排除规则。

- `src/api/nodes.rs`：92.31%（264 / 286 行）。
- 整个 `ait-server` 包在这组定向测试下：43.60%（1,118 / 2,564 行），不代表全套测试结果。
- 无可比覆盖率基线；旧代码只用于错误复现。先前 6 项测量未包含密码登录与删除主机回归，
  与当前 7 项测量范围不同，不能作为覆盖率变化基线。
- TypeScript 覆盖率未测量；本轮以定向回归和静态检查验证，提交准备时再按需要测量。
- Ait 根 Rust workspace 未测量、未运行测试，因为本轮没有修改其 `bins/` 或 `crates/` Rust 源码。

精确测量命令与逐文件统计保存于[共享覆盖率摘要](daemon-registration-coverage.json)：

```sh
DATABASE_URL=postgres://ait_registration_test@127.0.0.1:55473/ait_registration cargo llvm-cov --offline --locked -p ait-server --features integration-tests --test api --json --output-path /private/tmp/ait-server-registration-coverage.json -- relay::registration
```

未覆盖的注册模块行为主要是节点配额、安装身份被绑定到另一 runtime 的错误，
部分数据库失败和票据授权拒绝分支。这些不属于本次迁移路径；完整包的管理、CLI 和
其他业务行为未纳入这组定向测试。全套覆盖率留到提交准备，不以本轮结果宣称完整覆盖。

## 生效条件

本轮完成代码修复与本地验证，未部署在线中心或发布桌面安装包。
需要部署中心兼容修复并更新客户端；只更新客户端不足以处理已有随机或旧客户端绑定。
其中删除主机后的纯客户端登录修复只需部署中心，现有客户端已发送 `runtime = null`。
已有 runtime 的活动租约须先停止同步或等到租约失效，再由稳定 daemon 身份接入。
