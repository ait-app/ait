# Paseo 来源与许可证

客户端最初来自 `getpaseo/paseo`，版本 `0.9.0-beta.2`，提交
`2c8e8a826810337492cc5a38bb0bbd705b6fb632`。

| 上游目录           | 当前本仓库目录                  | 初次导入文件数 |
| ------------------ | ------------------------------- | -------------: |
| `packages/desktop` | [apps/desktop](../apps/desktop) |            205 |
| `packages/app`     | [apps/mobile](../apps/mobile)   |          2,564 |

目录最初按上游 Git 管理的源码、测试、资源和可执行权限导入。
[import-manifest.json](import-manifest.json)保存初始来源摘要与本地改动归属；摘要对应导入时的
上游内容，不表示当前源码未修改。目录重命名不改变这些来源摘要。

Ait 自有代码采用根目录的 [Apache License 2.0](../LICENSE)，与 Paseo 一致。
上游 Apache-2.0 许可证保存在 [third-party/paseo/LICENSE](../third-party/paseo/LICENSE)，
[客户端 LICENSE](LICENSE) 和 [桌面 LICENSE](../apps/desktop/LICENSE) 同样采用 Apache-2.0，并保留上游署名。
Rust 移植的署名与范围见 [NOTICE](../third-party/paseo/NOTICE)。项目内第三方许可证随原文件保留。

## 当前维护入口

Ait 客户端包使用私有 `@ait/*` workspace 与显式 `file:` 依赖；桌面和共享界面分别位于
`apps/desktop`、`apps/mobile`。SDK 位于 [packages/client](../packages/client/README.md)，
连接由 Rust daemon 与 transport adapter 实现。

```sh
npm ci
npm run verify:local-packages
npm run dev:desktop
```

当前开发、构建与运行方式见 [项目说明](../README.md)、[架构](../docs/architecture/README.md)
和 [daemon 手册](../docs/operations/daemon.md)。导入清单用于来源追溯，不作为当前功能说明。
