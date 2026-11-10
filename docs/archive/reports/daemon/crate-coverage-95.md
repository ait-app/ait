# Daemon 每 crate 95% 行覆盖率

本报告记录 rebase 前 `5c91c637` 的 12-crate 测量。分支已更新到 main `be9347f8`，
当前 13-crate 的验证与覆盖率见 [rebase 验证](crate-coverage-rebase.md)。

日期：2026-10-03。基线：main `6ce645040c0ef03d9d677ce9224c0fa9cdcb028b`。
测量对象为该提交加 `codex/daemon-crate-coverage` 的 Rust 改动；精确源码 SHA-256
及各文件计数见[可审阅覆盖率证据](crate-coverage-95.json)。

## 测试设计与修复

新增 118 项 Rust 测试，覆盖真实临时 Git 仓库、daemon/WebSocket、
本地 HTTP 和可控 Provider 子进程；外部服务使用夹具，不依赖已登录账号或下载语音模型。
用表驱动覆盖协议错误和状态映射，用通道与暂停时间控制取消、重放、部分结果和最终结果。
复用已有领域夹具，测试位于独立 child module，生产代码只抽出必要的私有输入输出边界。

测试扩展涉及配置与存储失败、图标格式、Git/Forge 错误、工作树回滚与恢复、
订阅删除和游标、Provider 原生控制、持久化、任务列表、语音 worker 生命周期和资源上限。
测试同时发现并修复以下行为：

- 文件浏览器的大目录 JSON 可以超过 4 MiB 队列并断连。复用 Checkout 的编码预算，
  返回目录过大的业务错误；真实 daemon 测试验证同一 WebSocket 随后的文件读取仍成功。
- 尚无首个提交的仓库读取历史时报错。现在返回空历史；同时验证新文件撤销后的索引和目录状态。
- Setup 命令在两次轮询之间快速输出超过 8 MiB 并退出时绕过上限。退出后读取同样有界，
  并返回明确错误；8 MiB + 1 字节用例在修复前失败。
- 离线 TTS 因样本上限中止后被误报为 Provider 故障。优先保留 Capacity 错误分类。

目录符号链接的回归用例验证允许目标按目录显示；该生产修复已包含在最新 main 中，
本 PR 保留回归测试。

离线引擎将编解码、容量和文本规范化与 native 调用隔开；worker 的协议循环接收
输入输出与引擎操作，使失败、重用、清理可以独立验证。没有改变领域归属或向外增加依赖。
DeepSeek 正常握手夹具的 2 秒等待在插桩与并行编译时曾失败，现放宽为 10 秒；
专门测试超时的 300 毫秒门槛保持不变，并已通过定向与最终全量复验。

## Test coverage

全部 **12 个 crate 均达到 95% 行覆盖率**。整体 **95.2289%（45,308 / 47,578）**，
可比基线为 92.3328%（43,871 / 47,514），提升 2.8961 个百分点。
覆盖率是行执行计数，不代表分支、真实外部模型或所有平台均已覆盖。

| crate      | 当前行覆盖率 | covered / total | 可比基线  |
| ---------- | ------------ | --------------- | --------- |
| api        | 95.7077%     | 1,650 / 1,724   | 95.7077%  |
| browser    | 97.3793%     | 706 / 725       | 97.3793%  |
| daemon     | 95.3863%     | 889 / 932       | 94.3133%  |
| domain     | 100.0000%    | 121 / 121       | 100.0000% |
| filesystem | 95.0171%     | 11,136 / 11,720 | 90.2647%  |
| metadata   | 95.0904%     | 7,360 / 7,740   | 90.7786%  |
| model      | 95.5994%     | 630 / 659       | 95.4476%  |
| protocol   | 100.0000%    | 57 / 57         | 100.0000% |
| provider   | 95.0471%     | 18,653 / 19,625 | 93.4217%  |
| schedule   | 97.8129%     | 805 / 823       | 97.8129%  |
| terminal   | 96.2202%     | 1,451 / 1,508   | 95.8886%  |
| voice      | 95.1646%     | 1,850 / 1,944   | 87.1970%  |

基线采用[主线仓库审计证据](repository-audit-2026-10-03-coverage.json)，其记录的 Rust 源码
与合并后的基线一致。原始整体为 43,974 / 47,707；原 terminal 的 `test_support.rs`
本已受 `cfg(test)` 保护，本次移到标准的 `tests/support.rs`，被 llvm-cov 默认测试过滤识别。
因此比较时从基线扣除该夹具的 103 / 193 行，terminal 的生产代码基线为 1,446 / 1,508。
夹具重排带来的分母变化没有计作生产覆盖提升。没有新增覆盖率排除规则或忽略测试。

环境：macOS arm64、Rust 1.98.1、cargo-llvm-cov 0.8.4、完整 Cargo workspace、默认 features、
默认文件过滤；不含 TypeScript 和 doctest 插桩。最终结果来自干净的单次全量运行，
没有使用累计运行结果；原始 JSON 中的全部源码路径均属于当前工作树。

```sh
SHERPA_ONNX_LIB_DIR=/private/tmp/ait-workspace-sidebar-cov-target/sherpa-onnx-prebuilt/sherpa-onnx-v1.13.8-osx-arm64-static-lib/lib CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov --locked --offline --workspace --html
CARGO_TARGET_DIR=/private/tmp/ait-workspace-sidebar-cov-target cargo llvm-cov report --json --summary-only --output-path /private/tmp/ait-crate-coverage/final.json
python3 scripts/check-crate-coverage.py /private/tmp/ait-crate-coverage/final.json
```

HTML 位于 `/private/tmp/ait-workspace-sidebar-cov-target/llvm-cov/html/index.html`。
共享证据为仓库中的 JSON 摘要：包含全部生产文件行计数、每个 crate、可比基线、测试数量
及 Rust 源码哈希。源码指纹为 `4bb7fa6c8e1105ded926bbff9f8adeae47ef4ae8bad2eb6d58aad9abfd7731c9`。

## 验证与本地检查

测试数量与覆盖率分别统计：普通全量和插桩全量均为 **1,690 passed、0 failed、3 ignored**；
基线为 1,572 passed，新增 118 项。
保留的 3 项 ignored 分别依赖已安装 Claude 模型发现、已认证 Claude 原生回合和 Codex 原生回合。

以下提交前检查通过，Rust 命令带 `--locked --offline`：

- `cargo test --workspace`、`cargo build --workspace`。
- `cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`。
- 本地覆盖率检查脚本与 2 项 Python 回归测试。
- 文档检查器测试、文档链接检查、修改文档与 CI YAML 格式检查、`git diff --check`。

普通构建使用 `CARGO_TARGET_DIR=/private/tmp/ait-git-fetch-target`，
`SHERPA_ONNX_LIB_DIR` 与覆盖率命令相同。

[CI](../../../../.github/workflows/ci.yml) 保持原有格式、Clippy 和全量测试检查，
不运行覆盖率检查，也不将 95% 覆盖率作为 PR 合并条件。
[本地检查脚本](../../../../scripts/check-crate-coverage.py) 可按需手动执行：从 Cargo metadata
枚举所有成员，按精确 covered/count 检查本次 95% 目标，缺失成员或零行报告也会报错。
JSON/HTML 报告由本地测量生成，仓库中的 JSON 证据保留本次验证结果。

尚未在本地执行 Linux CI、Windows 或原生客户端测试。未覆盖区域主要是 native 语音模型执行、
锁中毒、罕见文件系统/进程故障及平台清理分支；真实模型与已认证 Provider 需在具备环境时验证。
本次测量中部分 crate 接近 95%；后续改动可使用本地报告检查覆盖情况。

撤回 CI 覆盖率检查时仅修改了工作流和说明，Rust 源码与上述测量一致。
本次未重跑 Rust 测试或覆盖率；工作流格式、文档链接与本地检查脚本测试通过。
