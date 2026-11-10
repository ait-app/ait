# Ait / Paseo 共存修复验证

日期：2026-09-27。决策：[ADR-056](../../../decisions/clients/adr-056-paseo-coexistence.md)。

## 修改范围

| 冲突               | 修复后的行为                                                                                                             |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| 技能目录与卸载     | `ait-<name>` 目录加 `.ait-managed-files.json` 所有权标记；保留 Paseo、普通同名目录和用户文件；无所有权的命名冲突拒绝修改 |
| 技能事务恢复       | `ait-v1` 日志命名空间；拒绝旧无命名空间日志，保留日志和备份                                                              |
| Git 分支自动 stash | 创建和筛选 `ait-auto-stash:`；Paseo stash 保持原对象及内容                                                               |
| 桌面启动环境       | 所有桌面启动、profile、调试、开发、打包 smoke 变量使用 Ait 命名空间；不读取旧变量作为回退                                |
| Remote SSH         | 默认 7316，`/v1/ws`，Rust 协议协商与 Bearer 认证；主机存储支持令牌持久化和轮换                                           |
| 移动应用身份       | 生产 `dev.ait.mobile`、开发 `dev.ait.mobile.debug`；更新 Fastlane、Maestro 和设备测试脚本                                |

原有桌面用户目录、链接、浏览器分区和更新缓存隔离见
[桌面隔离报告](desktop-profile-isolation.md)。现有监听设置修改保持不变。

## 回归验证

- 桌面六个测试文件：45 通过，9 个 Linux 平台专用用例在 macOS 跳过。覆盖 profile、
  环境变量、SSH IPC 认证、CLI、shell 环境、开发配置及 Linux 打包资源门禁。
- 真实回环 stdio 隧道：1 通过。使用本地子进程替代 OpenSSH 连接，验证目标端口、
  `/v1/ws` upgrade、Authorization、消息来回传输和关闭清理；未使用远程主机或密钥。
- 前端七个测试文件合计 165 通过：主机保存与令牌轮换、探测配置、桌面桥接、
  Rust 多通道握手、项目 RPC、Host runtime、翻译资源及 SSH 令牌诊断脱敏。初次冷导入超过默认 5 秒，
  涉及的两个文件以 `--testTimeout 30000` 重跑，26/26 通过。
- 协议包：SSH / Agent 深链接 14 通过。
- Node 构建验证：5 通过，包括通过 Expo `getConfig` 加载生产和开发配置以及 ASAR 身份检查。
- protocol build、Electron main build、App TypeScript 检查通过。
- Oxfmt、Oxlint、修改过的 shell 脚本语法和 `git diff --check` 通过。

复现命令（分别在对应 workspace 执行 Vitest）：

```sh
npm run build:protocol
npm run build:main --workspace=@getpaseo/desktop
node_modules/.bin/tsgo --noEmit -p apps/mobile/tsconfig.json
node --test scripts/verify-packaged-resources.test.cjs apps/mobile/scripts/app-identity.test.cjs
# apps/desktop
../../node_modules/.bin/vitest run src/branding.test.ts src/daemon/local-transport.test.ts src/daemon/cli/passthrough.test.ts src/login-shell-env.test.ts src/daemon/linux-launcher.posix.test.ts scripts/dev-runtime.test.mjs
../../node_modules/.bin/vitest run src/daemon/local-transport-ssh.test.ts
# apps/mobile
../../node_modules/.bin/vitest run --project unit --testTimeout 30000 src/types/host-connection.test.ts src/utils/test-daemon-connection.test.ts src/desktop/daemon/desktop-daemon-transport.test.ts src/runtime/rust-daemon/transport.test.ts src/i18n/resources.test.ts src/runtime/host-runtime.test.ts src/diagnostics/app-diagnostic-report.test.ts
# packages/protocol
../../node_modules/.bin/vitest run src/ssh-transport.test.ts src/agent-deep-link.test.ts
```

## Test coverage

`cargo llvm-cov --workspace --html --output-dir target/coverage/ait-paseo-isolation` 通过，
执行 1,704 个测试，0 失败，8 个按原测试配置忽略（需外部凭据或特定运行条件）。
覆盖率范围为当前整个 Rust 工作区，包含已有改动，并非仅本次增量；TS 未采集覆盖率。

| 范围                 | 行覆盖率 | 已覆盖 / 可执行行 |
| -------------------- | -------- | ----------------- |
| Rust workspace       | 85.25%   | 57,642 / 67,613   |
| Skills 扫描与所有权  | 95.45%   | 168 / 176         |
| Skills 事务恢复      | 95.21%   | 159 / 167         |
| Skills 文件树        | 95.55%   | 236 / 247         |
| Checkout（含 stash） | 84.98%   | 1,228 / 1,445     |

Workspace 函数覆盖率 82.18%（6,104 / 7,428），区域覆盖率 83.35%
（83,262 / 99,894）；未启用分支覆盖率插桩。
机读数据：[覆盖率摘要](paseo-coexistence-coverage.json)；HTML 位于仓库
`target/coverage/ait-paseo-isolation/html/index.html`。

`cargo fmt --all --check` 和 `cargo clippy --workspace --all-targets -- -D warnings` 通过。
独立 `cargo test --workspace` 的沙箱初跑因回环端口权限失败，已在允许本地监听的环境重跑通过：1,705 成功（含 doctest），0 失败，8 忽略。

## 升级注意事项

本次只修改源码及测试，未替换 `/Applications/Ait.app`，未发布安装包。
已安装旧版需要重新构建、安装后才能生效。没有读取、删除或迁移用户的共享 Paseo profile、
技能目录或 Git stash；回归测试只操作临时目录和测试仓库。

旧 SSH 连接需要重新添加服务令牌；移动端新应用 ID 的签名和推送需单独配置。
没有执行 iOS/Android 原生签名或真机安装，也没有连接真实远程 SSH 服务。
旧技能目录不自动接管；旧事务若尚未完成，必须先确认备份与目录归属再恢复。
