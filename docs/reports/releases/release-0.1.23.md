# Ait 0.1.23 发布说明与验证

## 发布范围与来源

上一正式版 0.1.22 的构建来源为 `20baa5271d96ad34eef2940896d4437f28837cad`。
本次功能基线为 main `8fed61fbb57c118d1632c9372499eb9a9ffb63b9`，纳入 #526、#445、#527、#528、#525、#522、#532、#533、#534、#535。
发布 Linux/macOS/Windows 桌面、两个独立 daemon 及既有工作流的网页客户端镜像。
GitHub Draft 附件校验完成后暂停；收到本次继续指令后，再公开 Release、上传 COS、最后发布 macOS 清单并同步两个下载入口。
本次不触发 iOS/Android 工作流；原生版本和本地 Pod 锁文件仅作为统一源码元数据同步。

## 更新内容

- 在线业务连接统一使用 Hub，移除传统 Relay 业务回退；专用文件下载通道保留。仅支持旧 Relay 的中心或 Host 无法建立本版在线业务连接。
- 开启 MCP 注入后，Codex、Claude、DSH ACP、MiniMax Code 的新建和恢复会话自动连接 daemon 编排 MCP；保留已有显式关闭和用户同名配置。
- macOS/Linux 的 MCP 支持可选 Unix socket 监听，默认关闭；继续使用相同 HTTP MCP 协议和 Bearer 鉴权。
- 统一侧栏工作区排序并移除拖拽排序，修复部分浏览器的内部 transport URL 解析。
- 隐藏 Claude 注入的子 Agent 交回记录，将交回报告显示为消息；通过 npm 配置发现 DSH 安装。
- Vim 写文件命令触发实际保存；过滤实时语音上下行静音 PCM；Rust 异步接口统一使用 async-trait。

## 版本与验证

15 个 Cargo 包、7 个 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.23；iOS 源码版本为 0.1.23 / 1023999。
版本准备不修改 Rust 实现或第三方依赖。

- `npm ci`、`npm run verify:release` 通过。
- `npm run test:release`：75 项通过；`npm run test:mobile-release`：37 项通过。
- 文档检查、格式检查、桌面主进程构建通过；生成新的报告后再次核对格式与文档链接。
- 使用生产身份 Expo prebuild 生成原生元数据，CocoaPods 1.16.2 的 `pod install` 与 `pod install --deployment` 通过；仅保留版本字段、本地 Pod 版本和 checksum，无关生成资源已恢复。
- [功能基线 CI](https://github.com/ait-app/ait/actions/runs/41078494958)、最终合入 main 的 CI、三平台打包和成品门禁的最终结果在 Release Notes 及发布工作区记录；全部成功后才交付 Draft。
- 按 AGENTS.md，版本准备没有 Rust 实现变更，未运行本地 Rust 测试。

具体命令：

```sh
npm run verify:release
npm run test:release
npm run test:mobile-release
npm run check:docs
npm run check:format
npm run build:desktop-main
```

## Test coverage

未测量（not measured）：本次未请求覆盖率，测试通过数不代表覆盖率；未运行本地全量 Rust workspace 测试。后续需要时可对实际改动范围使用 `cargo llvm-cov` 生成可审阅报告。
本次版本准备未进行生产账号授权与跨网重连、真实模型、移动设备或已安装 Electron 升级的端到端验收。
Windows 包未签名、不含 daemon，需要远程 Host；网页客户端镜像发布与生产部署分别记录。
