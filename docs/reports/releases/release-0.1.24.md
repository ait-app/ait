# Ait 0.1.24 发布说明与验证

## 发布范围与来源

上一正式版 0.1.23 的实际构建来源为 `5fdc2c96b9b08c8a3dc8bc89a487643d687cf51d`，与保留的原标签不同。
本次功能基线为 main `4cb5b55ef9901c16411c0b764596f152c14d736d`，纳入 #539、#540、#541、#542、#538、#543、#544、#530、#545。
发布 Linux/macOS/Windows 桌面、两个独立 daemon，以及既有工作流中的网页客户端镜像。
本次在 GitHub Draft 附件校验完成后暂停；收到本次继续指令后才公开 Release、同步 COS 和两个下载入口。
Draft 阶段下载桌面/COS 必需的 10 个文件：macOS 安装与更新文件、两个 daemon、一个 Linux DEB、构建来源与校验清单；Windows 和其他 Linux 格式只核对官方 CI 与远端摘要。
本次不触发 iOS/Android 构建或商店提交；仅同步统一源码版本和原生工程元数据。

## 更新内容

- 支持将工作区、项目或整个节点分享给群组成员，通过 Hub 访客会话访问；实现分享撤销、权限校验与离线恢复。
- 修复项目/工作区共享目录持续加载，整理共享与主机导航、外观设置及中文计划文案；首页账号入口打开统一登录中心，Homepage 指向 Ait 官网。
- 恢复原生与嵌套子 Agent 列表，补齐 OpenCode 子 Agent 状态、历史恢复和实时更新。
- 处理 Codex 子 Agent 完成通知时保持父会话运行，修正 Provider 执行归属及公共 API 契约。
- 改善 Pi 可用性与版本探测诊断，标记由 Ait 启动的 Claude 会话。
- 修复 macOS 侧边栏切换与首页链接被窗口拖动区域拦截。

资源共享需要中心声明相应能力和支持分享管理的新版 daemon。工作区/项目访客按资源范围与创建者校验操作，节点分享具有节点管理权限。终端与 Agent 以 Host 用户身份执行命令，资源范围不等于操作系统沙箱；详见 [ADR-203](../../decisions/daemon/adr-203-workspace-share-guests.md)。

## 版本与验证

15 个 Cargo 包、7 个 npm 包、锁文件、PKGBUILD 和 CHANGELOG 同步为 0.1.24；iOS 源码版本为 0.1.24 / 1024999。
版本准备没有修改 Rust 实现或第三方依赖。

- 锁定依赖安装、源码版本一致性、发布工具与原生元数据定向测试、文档和格式检查、桌面主进程构建作为本次提交前检查；精确结果在版本 PR 与发布工作区记录。
- 使用既有生产身份生成原生元数据，CocoaPods 1.16.2 的安装与 deployment 模式检查通过后，仅保留版本字段和本地 Pod 版本/checksum，无关生成资源恢复。
- [功能基线 CI](https://github.com/ait-app/ait/actions/runs/41220150915)、[资源共享主提交 CI](https://github.com/ait-app/ait/actions/runs/41215482743)、最终合入 main 的 CI 和正式三平台构建的最终状态写入 Release Notes；全部成功后才交付 Draft。
- 按 AGENTS.md，本次版本准备没有 Rust 实现变更，未运行本地 Rust 测试。

```sh
npm ci
npm run verify:release
npm run test:release
npm run test:mobile-release
npm run check:docs
npm run check:format
npm run build:desktop-main
```

## Test coverage

未测量（not measured）：本次未请求覆盖率，测试通过数不代表覆盖率；未运行本地全量 Rust workspace 测试。
版本准备没有执行生产账号、真实模型、完整新旧客户端共享矩阵、移动设备或本机已安装应用升级的端到端验收；其他 PR 的历史测试记录不视作本次构建的实机复验。
Windows 包未签名、不含 daemon，需要远程 Host；网页客户端镜像发布与生产部署分别记录。
