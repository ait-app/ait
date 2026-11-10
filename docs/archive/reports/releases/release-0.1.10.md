# Ait 0.1.10 发布说明与验证

## 发布范围与源码

上一正式版 `v0.1.9` 的当前 GitHub `BUILD-INFO.json` 及 API digest 已重新核对，
实际交付源码为 `83a584e7b0334307385350521e345511ef631985`。
本次纳入 main 至 `5beb646d87ee311e9f6af653bc6e8cd683bf3f1f` 的功能、修复及发布流程更新。
最终不可变标签为 `v0.1.10`，源码和工作流运行以本次成品 `BUILD-INFO.json` 为准。

本次仅发布桌面、COS、官网及服务端下载入口。用户明确要求本次不发布 iOS 或 Android；
不触发移动端 EAS 构建、TestFlight 或 Google Play 提交。活跃 npm workspace 的版本仍按桌面发布要求统一，
共享界面的测试不构成移动应用发布。

## 更新内容

- 统一 Markdown 引擎，保留高亮、代码复制、Mermaid、查找、附件和远程图片，改进文件及目录链接、相对图片解析。
- 改进长提示词编辑、草稿与选区生命周期、Diff 评论反馈、辅助面板、Scripts、Command Center 排序与子 Agent 信息。
- 补全聊天 catch-up 的历史尾部，按实体隔离缓存失效，优化长聊天投影和布局复用，修复宽字符终端选择与滚动。
- 用量刷新失败时保留同一已知账号的上次成功值；Setup 详细日志只在活动可见面板订阅，重连、revision 与释放按连接隔离。
- 修复 Git quoted path、worktree 根目录和 rename diff/discard；运行中的定时任务阻止编辑时提供明确解释。
- 后台错误保留可复制诊断并减少打扰；浏览器默认打开 Ait 官网；Linux 窗口显示增加兜底，关闭的 console 管道忽略 EPIPE。
- 稳定桌面 Release 自动发布 AUR `ait-bin`；Play 观察窗口延长至 60 分钟，但本次不执行移动端发布。

## 发布准备与验证

测量与验证提交为 `2e49c54ad817493547b35dbe9de568aa7c9227ea`。

- 15 个本地 Cargo 包、7 个 npm 包、两个锁文件、根 PKGBUILD 与 CHANGELOG 同步为 0.1.10；核对 npm 锁文件结构，第三方依赖与发布基线一致。
- `npm ci`、`node scripts/verify-release-version.mjs v0.1.10`、`npm run test:release`（60 项）和 `npm run test:mobile-release`（41 项契约测试，无发布操作）。
- `npm run build:desktop-main`、`npm run typecheck --workspace=@ait/desktop --workspace=@ait/mobile`、文档链接和变更文件格式检查通过。
- 本次变更的 59 个共享界面单元测试文件，1206 项通过；5 个真实 Chromium 文件，72 项通过。
- 桌面窗口与 console 管道 2 文件 8 项；协议展示与终端快照 3 文件 39 项；连接订阅 2 文件 46 项通过。
- 新功能来源 [PR #394](https://github.com/ait-app/ait/pull/394) 的 [CI](https://github.com/ait-app/ait/actions/runs/39290245339)已成功。
- 本次准备仅修改版本元数据、CHANGELOG 和发布证据；按根 AGENTS.md，本地未重跑 Rust 测试或覆盖率。适用的 Rust 检查由源码 PR 和最终发布提交的 CI 独立核验。

正式发布前等待发布准备 PR CI、适用 main 检查和两平台正式构建、签名、公证及成品生命周期门禁；
完整下载核对 14 个桌面附件及 SHA256SUMS、自动更新元数据、固定标签和 BUILD-INFO。
之后使用跨仓库发布编排顺序上传并验证 COS 文件，最后更新根 JSON，分别验证官网部署和服务端生产下载入口。
本报告记录准备证据，公开下载及实际线上状态由 Release Notes 与跨仓库交付报告补充。

## Test coverage

本次在 macOS arm64、Node 26.11.1、Vitest 4.1.7 下，使用隔离安装的
`@vitest/coverage-v8 4.1.7` 实测 **199 个共享界面变更 TS/TSX 文件的单元测试行覆盖率**：
**27.68%（4980/17991 行）**。测试为本次变更的 59 个 unit 文件，1206 通过、0 失败、0 跳过，
保持项目原 worker 配置。5 文件 72 项 Chromium 测试单独通过，其执行量未合并进该覆盖率。

[共享 JSON 证据](release-0.1.10-coverage.json)包含 199 个文件的行、分支、语句与函数计数、
未执行行、完整实际参数列表以及 2526 个源码和配置输入的 SHA-256。
测量提交为 `2e49c54ad817493547b35dbe9de568aa7c9227ea`，后续只增加文档和证据，正式交付前核对输入指纹。

精确命令的参数数组见证据 `commands.coverageArgv`，工作目录为 `apps/mobile`：

```sh
npm install --prefix /tmp/ait-release-0.1.10-coverage-tools --no-audit --no-fund --ignore-scripts @vitest/coverage-v8@4.1.7
NODE_OPTIONS=--no-experimental-webstorage npm exec --no -- vitest run --project unit \
  --coverage --coverage.provider custom \
  --coverage.customProviderModule /tmp/ait-release-0.1.10-coverage-tools/node_modules/@vitest/coverage-v8/dist/index.js \
  --coverage.reportsDirectory /tmp/ait-release-0.1.10-js-coverage \
  --coverage.reporter json --coverage.reporter json-summary --coverage.reporter html \
  --reporter json --outputFile /tmp/ait-release-0.1.10-ui-results.json \
  <证据中列出的 199 个 --coverage.include 参数和 59 个测试路径>
```

范围由已交付 0.1.9 至固定 main 基线的现存 `apps/mobile/src` 非测试 TS/TSX 文件确定，
排除类型声明并沿用 Vitest 默认测试/依赖排除。大量变更的页面、容器和原生平台代码计入分母，
但没有被这些定向 unit 用例挂载，因而存在重要未覆盖行为：原生键盘、选区、手势与动画，
以及完整页面集成。没有宣称整个产品覆盖率或移动端真机验收。

Rust 覆盖率本次未测量，未复用历史数据；发布准备不修改 bins/crates 中的 Rust 代码，
根 AGENTS.md 要求跳过相应本地测试。上一版本覆盖率属于不同的 Rust 范围，
无可比 JavaScript/TypeScript 基线，不推算百分点变化。
未执行用户已安装客户端的真实替换升级，也未进行 iOS/Android 真机验收。
