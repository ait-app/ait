# Workspace 重命名与文档整理

源码基线：`3ab58e57` 加本次工作区改动；平台：macOS arm64。
决策见 [ADR-072](../../../decisions/daemon/adr-072-workspace-names-and-documentation.md)。

## 改动

- 服务 package、binary 与目录统一为 `daemon`，11 个能力 crate 移除 `server-` 前缀。
- Electron 和 Expo 目录改为 `apps/desktop`、`apps/mobile`，包名为 `@ait/desktop`、`@ait/mobile`。
- 更新启动、安装包、签名、CI、开发脚本、锁文件和跨应用 E2E 引用。
- 依赖守卫检查全部 workspace 包；显式区分短 crate 名与局部同名模块。
- 发布工具从源码识别布局，支持当前 daemon 和改名前的不可变标签。
- 清理已移除实现的文档，分类 ADR 与报告，重写当前索引、架构与 daemon 手册。
- 新增本地 Markdown 链接检查及 CI 入口；保留第三方来源和许可证。

环境变量、已有数据目录、身份文件和公开 wire 名称保持兼容。
本次未修改第三方依赖版本；没有删除或迁移用户数据。

## 验证

- `cargo fmt --all --check`、`cargo clippy --workspace --all-targets --offline -- -D warnings`、
  `cargo build --workspace --offline` 通过，构建无警告。
- `cargo test --workspace --offline`：1,558 项通过，3 项按既有规则忽略。
- 桌面 Vitest：53 个文件、377 项通过；9 项 Linux 专属用例在 macOS 跳过。设置
  `AIT_SERVER_BIN` 指向真实 `target/debug/daemon`，验证启动、认证、重启和停止。
- 移动端 CI 定向集合：107 项通过；额外启用真实 daemon 适配器集合：56 项通过。
- 桌面平台 Web 导出成功，输出 `apps/mobile/dist`。
- Electron 真实启动检查通过：使用该 Web 导出、本机临时静态服务与离线 Codex fixture，
  验证 daemon 自动启动、认证、Terminal、重启重连、Agent timeline 与退出清理；渲染进程无错误。
- 本地 protocol/client 测试：749 / 216 项通过；两端 TypeScript 检查通过。
- 发布脚本 15 项、移动发布脚本 16 项通过；真实 daemon 版本、暂存及可执行权限检查通过。
- Playwright 文件加载：移动/Web 624 项、桌面 19 项；只验证收集与跨目录引用，未执行全部 UI 用例。
- `npm run check:docs` 验证 165 份 Markdown 的本地链接；品牌 38 项资产校验通过。
- 修改的 JS/TS、JSON、YAML、Markdown 经过 Oxfmt；修改的 JS/TS 经过 Oxlint，无警告或错误。

本机依赖目录由其他 checkout 的符号链接组成，修复本 worktree 的 workspace 链接，并为 Metro
本地化嵌套依赖后执行 Web 导出。开发构建不修改已锁定第三方版本。
Rust 语音依赖使用已有 Sherpa ONNX v1.13.8 静态库缓存。回归涉及本机端口、PTY 与 fixture
子进程，最终在允许这些操作的环境中执行；沙箱内因监听权限失败的初轮不算通过。

## Test coverage

完整 workspace 行覆盖率 **92.15%（43,948 / 47,691 行）**。无直接可比的改动前测量。
测量范围为全部 12 个 package、默认 features，沿用 cargo-llvm-cov 默认测试代码排除，
没有额外源码或 crate 排除。macOS arm64；Linux/Windows 与真实移动设备未测量。
3 项既有 ignored 测试仍跳过，真实 Provider 验证未启用。测试通过数与覆盖率分别记录。

复现命令：

```sh
export SHERPA_ONNX_LIB_DIR=/path/to/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib
cargo llvm-cov --workspace --html --offline
cargo llvm-cov report --json --summary-only --output-path .tmp/rename-coverage-summary.json --offline
```

源码为 `3ab58e57` 加本次 Rust 重命名；可审查的
[覆盖率摘要及各源码 SHA-256](workspace-rename-coverage.json)记录命令、平台、每个 crate 和文件的
实际行数。完整 HTML 已生成到 `target/llvm-cov/html/index.html`，未提交运行产物。

| Package    | 覆盖行 / 总行   | 行覆盖率 |
| ---------- | --------------- | -------- |
| api        | 1,650 / 1,724   | 95.71%   |
| browser    | 706 / 725       | 97.38%   |
| daemon     | 879 / 932       | 94.31%   |
| domain     | 121 / 121       | 100.00%  |
| filesystem | 10,547 / 11,697 | 90.17%   |
| metadata   | 7,019 / 7,732   | 90.78%   |
| model      | 629 / 659       | 95.45%   |
| protocol   | 57 / 57         | 100.00%  |
| provider   | 18,334 / 19,625 | 93.42%   |
| schedule   | 802 / 820       | 97.80%   |
| terminal   | 1,549 / 1,701   | 91.06%   |
| voice      | 1,655 / 1,898   | 87.20%   |

剩余 3,743 行主要是 I/O 失败、外部 Provider/语音后端和平台特定分支；没有为了覆盖率改变无关行为。
正式安装包的签名、公证、Linux 运行和真实 iOS/Android 构建需在对应发布环境验证。
