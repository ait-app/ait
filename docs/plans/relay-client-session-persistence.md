# Relay 客户端登录保持

排查日期：2026-10-10。客户端基线 `1ea12ee6`，同级 `ait-server` 源码基线
`fc69941`。以下服务端结论来自本地源码，未检查线上部署或真实用户令牌。

## 原问题与现有约束

- 桌面、Android、iOS 共用
  [AccountSessionManager](../../packages/client/src/account-session.ts)。登录只保存
  `access_token` 与 `expires_in`，没有用户会话 refresh token。`restore()` 不恢复过期令牌，
  运行期间 `tick()` 遇到令牌过期也会注销。节点 `/renew` 只续节点租约，不延长用户登录。
- 修改前中心 `src/passwords.rs` 的 `SESSION_SECONDS` 固定为 8 小时。统一登录在
  `src/authing/mod.rs` 中还把 `oidc_sessions.expires_at` 限制为
  `min(Authing ID token exp, 当前时间 + 8 小时)`；原生兑换签发的 AIT 令牌也受此限制。
  因此实际会话可能短于 8 小时，隔夜启动通常必须重新认证。
- 移动端 [SecureStore 恢复](../../apps/mobile/src/runtime/native-account.native.ts)
  会删除过期记录；桌面 [safeStorage](../../apps/desktop/src/daemon/account-ipc.ts)
  保存加密记录。凭据已经持久化，单纯增加「记住登录」开关不能解决令牌到期。
- 桌面在系统安全存储不可用时不会落盘凭据；Linux 的 `basic_text` 也被排除。
  如果登录后立即重启就丢失状态，需要另查该设备的安全存储可用性。
- 原 `shutdown()` 先调用会删除凭据的 `logout()`，再写回旧凭据；中途被终止或写回失败会
  丢失本来有效的会话。这是另一处退出风险，并非移动端频繁登录的共同原因。

## 本次已修复

同级 `ait-server` 将网页、桌面、Android、iOS 的用户登录统一为最长 14 天。
实际期限取 `min(当前时间 + 14 天, Authing ID Token exp, 本地账号 expires_at)`，
账号无限期时省略最后一项；OIDC 会话与 AIT JWT 采用同一上限。
保留旧密码处理函数时也遵守 14 天及账号期限，不重新启用已禁用的登录路由。
新账号默认只有 7 天有效期，不因登录而变为 14 天。

关闭应用复用连接清理，但不删除或重写已经保存的账户。重新启动仍由 `restore()` 尝试
释放旧节点激活，再注册新激活。主动注销继续清除凭据，账户/令牌到期检查继续生效。
此改动保持现有平台凭据与 daemon 票据边界，不引入新的认证协议。

中心 Web 控制台改为在同源 `localStorage` 保存 JWT，迁移旧标签页凭据。重开页面恢复保存的
令牌，启动断网或服务临时失败不会删除它；主动退出或 401 清除当前令牌。已有标签页继续使用
自己的内存会话，旧标签页不能删除其他标签页后来保存的不同令牌。禁止浏览器存储时仅保持
当前页面登录。中心决策记录在其 `docs/decisions/adr-002-persistent-user-sessions.md`。

## 发布与生效

- 本次为源码实现及本地验证，尚未部署线上或修改 Authing 控制台。
- 发布 `ait-server` 后端与网页，发布带退出保护的客户端；本次不需要新增数据库迁移。
- Authing 的 `id_token` 期限设为 `1209600` 秒（14 天），上线时在控制台核对。
  授权码 600 秒、Access Token 14 天、Refresh Token 30 天无需为本次修复再调整。
- 旧 JWT 不会自动延期，发布后重新登录一次才能取得新期限；实际时间仍受提供商和账号限制。
- 这不是滑动续期，到期后仍需登录。验收步骤见[统一登录配置](../operations/authing-client-login.md)。

## 长期方案（待实施）

建议增加中心管理的客户端会话：短期访问令牌（例如 15 分钟）加可轮换的续期凭据
（例如 30 天会话期限）。这是待实现的协议，当前客户端和中心均未接入。

1. 中心在原生 PKCE 兑换后创建可独立撤销的客户端会话，返回访问令牌、续期凭据和各自期限。
   若继续支持原密码客户端登录，也应接入同一流程。服务端只存续期凭据摘要，提供轮换、
   撤销及失败重试的幂等机制；保留停用、账号到期、身份提供商撤销与密码重置的失效语义。
2. 明确定义客户端会话与 Authing 登录证明的期限关系。仅延长 AIT JWT 不够：当前
   `auth::resolve_user` 和节点授权还会校验原 OIDC 会话期限。新会话的访问校验和节点租约
   续期必须一起调整，避免新令牌可用但旧节点激活仍受旧访问令牌期限限制。
3. 共享客户端管理器在启动、回前台和访问令牌即将过期时续期，合并并发刷新请求，并可靠保存
   轮换结果；移动端允许恢复「访问令牌过期、续期凭据仍有效」的记录。网络故障保留凭据并
   重试，确定被撤销/到期才要求重新登录。UI 与 daemon 继续只接收非秘密快照或短期票据。
4. 明确每设备登出与其他主机同步的关系；不能撤销仍被其他显式同步 daemon 使用的授权。
   中心已有 `/auth/device/refresh` 属于无头机器授权，不能直接作为用户账户续期接口。
5. 先部署兼容旧客户端的中心，再发布共用会话实现。旧记录没有续期凭据，升级后需重新登录
   一次建立新会话。覆盖隔夜重启、断网重试、令牌轮换中断、多个客户端、手动登出、账号撤销
   及到期。实际 Android/iOS 安全存储与桌面密钥环需在安装包上验收。

## Test coverage

本仓库未修改 `bins/` 或 `crates/` 中的 Rust，按仓库规则不运行本仓库 Rust 测试。
TypeScript 行覆盖率 **not measured**：当前未安装 Vitest 覆盖率 provider，本次提交沿用已完成的
退出、恢复和相关登录定向测试，没有覆盖率产物或可比基线。后续接入覆盖率采集后补测这些路径；
真实中心及设备端到端验收仍待执行。

验证范围为上述客户端基线加本次工作区改动，运行环境为 Linux；未运行完整测试套件。

| 命令                                                                                                                                                                 | 结果                                                                                                             |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `npm run test --workspace=@ait/client -- src/account-session.test.ts src/account-browser-login.test.ts`                                                              | 46 项通过                                                                                                        |
| `npm exec --workspace=@ait/desktop -- vitest run src/daemon/account-session.test.ts src/daemon/account-ipc.test.ts src/daemon/account-browser-login.test.ts`         | 17 项通过；回环监听在沙箱内受限，获准在沙箱外重跑通过                                                            |
| `npm run test --workspace=@ait/mobile -- --project unit src/runtime/native-account.native.test.ts src/runtime/account-browser-login.native.test.ts`                  | 23 项通过；覆盖 Android/iOS 保存凭据恢复、离线启动后的自动重试                                                   |
| `npm run build:sdk`                                                                                                                                                  | 通过                                                                                                             |
| `npm run typecheck --workspace=@ait/client --workspace=@ait/desktop --workspace=@ait/mobile`                                                                         | client、desktop 通过；mobile 在未修改的 `src/app/_layout.tsx:1` 报 `lucide-react-native` 未导出 `LucideProvider` |
| `npm exec -- oxfmt --check packages/client/src/account-session.ts packages/client/src/account-session.test.ts apps/mobile/src/runtime/native-account.native.test.ts` | 通过                                                                                                             |
| `npm exec -- oxlint packages/client/src/account-session.ts packages/client/src/account-session.test.ts apps/mobile/src/runtime/native-account.native.test.ts`        | 无警告或错误                                                                                                     |
| `npm run check:docs`                                                                                                                                                 | 通过                                                                                                             |

以上 86 项通过数是测试执行结果，不代表行覆盖率。

共享客户端的 46 项测试在加入「14 天登录、隔夜重启且不改写到期时间」用例后重跑通过。
桌面、移动端结果为本次任务前段的定向验证，相关实现和测试此后未改动。

同级中心 `fc69941` 加本次改动通过期限单测 3 项、密码单测 3 项、Authing 集成测试 27 项、
密码相关集成测试 12 项（与 Authing 部分重叠），网页会话/登录定向测试 33 项。
中心 `cargo fmt --all --check`、带 `integration-tests` 的 Clippy `-D warnings`、网页
typecheck/build 和改动文件 Prettier 检查通过。具体命令及测试范围在中心 ADR-002 中记录。
中心测试使用模拟提供商和临时数据库；未测真实 Authing、浏览器安装环境或手机安装包。
