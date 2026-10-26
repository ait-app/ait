# ADR-192：Codex 历史按页投影后计算展示预算

- 状态：Accepted
- 日期：2026-10-26
- 修订：[ADR-191](adr-191-codex-native-realtime-voice.md) 的原始历史字节限制；延续 [ADR-157](adr-157-codex-user-image-projection.md) 的图片落盘、[ADR-140](adr-140-codex-mcp-display-previews.md) 的工具摘要和 [ADR-185](adr-185-native-provider-failure-details.md) 的失败说明。
- 问题：[Issue #491](https://github.com/ait-app/ait/issues/491)。

## 背景

Canonical 历史先累计所有原始 JSON，再投影图片和工具结果。超过 32 MiB 的合法历史
会在转换前失败。此前还会先调用 `thread/read(includeTurns: true)`，同时保留普通历史、
canonical 原始历史和展示投影。消息提交将历史读取错误统一映射为注册表故障，无法解释原因。

## 决策

1. `thread/read(includeTurns: false)` 只读取元信息。支持 `thread/timeline/list` 时，
   每次读取一页，立即使用共享投影将图片落盘、工具结果转换为有标记的摘要，然后释放原始行。
   RPC 结果按所有权移出响应，不复制整份 JSON；MCP 投影不再复制即将被替换的原始结果。
2. 32 MiB 限制仅计算已投影 `NativeItem` 的序列化字节，计数不生成第二份序列化缓冲区。
   保留 32768 个 canonical 条目和 512 页的上限，分页游标限制为 4096 字节。
   原始图片和工具输出总量不再作为展示历史拒绝条件。单个上游响应仍由现有传输完整解析，
   因此峰值内存仍受最大单页影响，不能声称与原始页面大小无关。
3. 页面仍由新到旧返回、页内保持正序。跨页严格校验递减 position，再按时间顺序消费轻量索引。
   普通消息只保留一份展示数据，promotion 使用相同的源身份；所有语音转写先建立索引，
   最后统一执行交接去重。跨页 turn 时间戳、先于源消息的 promotion、语音交接均保留。
4. 只在第一页明确不支持 canonical 方法时读取完整普通历史。预算、协议或其他 RPC 失败
   不静默回退，避免丢失语音与 delegation 顺序。已有展示缓存保留；本次不增加跳过校验的写入路径。
5. 历史错误携带原生方法、页码、累计展示字节，以及独立的预算、协议或 RPC 原因。
   超限错误包含资源名、限制和实际值。内容由适配器生成，不包含会话文本、路径或凭据。
   消息准入保留这些说明供已有持久错误机制展示，不再冒充注册表失败。
6. 读取和失败恢复不修改原生会话，不自动补发输入。沿用消息回执的保守重试策略，
   明确提示原生历史未改变，并可在解决具体错误后重试或使用原生客户端继续。

## 后果

图片较多的长历史可以在原始体积超过 32 MiB 时重开。适配器保留的原始数据按页释放，
跨页仅保留有界展示数据和关联索引。展示数据本身、条目数或页数仍可能达到限制，此时明确失败。
未新增依赖、数据库 schema 或客户端协议；原生历史仍由 Codex 拥有。

## 验证与 Test coverage

定向回归覆盖超过 32 MiB 的图片历史、单页超大工具载荷、展示预算精确边界、分页顺序、
重复 position / 消息、游标循环、旧版方法回退、语音关联，以及重启后已完成／中断会话继续输入。
集成测试同时检查原会话保留、重复消息键不产生新 turn、历史失败说明与后续恢复。
验证使用离线 stdio 原生协议 fixture，不调用模型；未验证 Linux AppImage 或报告者的私人会话。

验证版本为 `760d5614` 加本次工作区变更，环境为 macOS arm64、Rust 1.98.1。
相关测试 319 通过、3 个真实登录／模型调用用例按既有配置忽略；未执行完整 workspace 测试。
Clippy、Rust／Markdown 格式、Python fixture 语法、diff 空白检查及本地文档链接检查通过。
测试需允许回环网络及进程检查，并使用规范临时路径避免 macOS `/var` 与 `/private/var` 别名差异。
本机通过临时目录中已有的 CMake 构建 bundled Opus，关闭不可用的 sccache wrapper：

```sh
TMPDIR=/private/tmp PATH=/private/tmp/ait-cmake/bin:$PATH RUSTC_WRAPPER= \
  cargo test --offline -p provider --lib --locked -- \
  local::codex local::tool_detail::mcp service::agent_manager::tests \
  service::agent_execution::tests::delivery service::agent_execution::tests::resume \
  ports::agent_session::tests --format=terse
PATH=/private/tmp/ait-cmake/bin:$PATH RUSTC_WRAPPER= \
  cargo clippy --offline --locked -p provider --all-targets -- -D warnings
cargo fmt --all --check
npm run check:docs
```

图片回归使用 34 张 512 × 512 BMP、130 条消息和两页原生结果，原始 JSON 超过 32 MiB，
展示投影小于 64 KiB。测试 peer 在第二页请求时确认第一页图片已落盘，防止退回整份历史缓冲。
这是对按页释放和展示保留量的验证，不是 daemon 峰值 RSS 测量。

覆盖率未测量：用户未请求。若后续需要，使用 `cargo llvm-cov` 仅测量相关 provider 模块和回归，
记录对应 revision、行数及可共享的报告；不因本次修复运行完整 workspace 测试或覆盖率。
