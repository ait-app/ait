# 图片输入自动压缩、分片与接纳预算修复

日期：2026-10-13。修复基于拉取上游后的 `origin/main`
`13ce800ccb31d60a492338047a3c2f83d1c694c9`。平台：macOS arm64。

## 复现与原因

桌面日志连续记录 `Failed to send message: Error: Resource budget exhausted`。
失败前保存的 PNG 大小为 3,886,681 字节，base64 编码后为 5,182,244 字节；
检查时没有 queued 输入。仅记录大小，不提交用户图片、运行数据库或认证信息。

`AgentPrompt::validate` 接受最大 63 MiB 的序列化输入，但
`Timeline::reserve_input` 对单个 Agent 的 queued 输入只允许 4 MiB。
发送到已有 Agent 的图片必须先经过这条持久化接纳路径，所以合法的图片也会被空队列拒绝。
创建 Agent 时的 initial prompt 采用另一条路径，不能用创建成功证明后续发送支持同等大小。

新增的存储回归和离线 Codex 发送回归在修复前均失败，分别返回
`ResourceExhausted` 和 `accepted=false`；输入校验本身成功。
回归用相同大小的合成 base64 数据，不调用真实模型，也不修改正在运行的会话。

## 修复

- 提取既有 63 MiB 输入上限为共享常量，单 Agent 的 queued 字节预算与它一致。
- 全局 queued 字节预算为四个最大合法输入，即 252 MiB；单 Agent 仍最多 32 条输入。
- SQLite 聚合使用 BLOB 字节长度，避免中文等 UTF-8 内容被按字符数少计。
- 真正耗尽队列时记录数量、输入字节数、现有占用与上限，不记录消息或图片内容。

仅调整 Provider 内部接纳预算；没有协议字段、数据库 schema 或领域依赖边界变更。
重复请求仍复用原有 durable receipt，已提交但结果不确定的输入仍不会自动重放。
提高容量会允许更多待投递数据留在 SQLite 中，但队列仍有全局、单 Agent 和数量上限。

## 定向验证

Rust 1.98.1，Cargo 默认 features。范围为输入校验、持久化 inbox、rich input 和发送生命周期。
完整工作区测试和覆盖率没有运行；用户没有要求这两项全量检查。

19 项定向测试通过，818 项不相关测试跳过。最大容量测试的夹具随后改为复用已接纳的
持久化输入，避免重复解码和序列化；该测试再次单独通过（11.44 秒）。
Provider 全 targets Clippy、完整工作区构建、Rust 和 Markdown 格式、Diff 与文档链接检查通过。

```sh
nix develop --command cargo nextest run --locked --offline -p provider --lib -E 'test(storage::timeline::inbox::tests::) | test(protocol::prompt::tests::) | test(service::agent_execution::tests::rich_input::) | test(service::agent_execution::tests::delivery::)'
nix develop --command cargo clippy --locked --offline -p provider --all-targets -- -D warnings
nix develop --command cargo build --locked --workspace
nix develop --command cargo fmt --all --check
git diff --check
node scripts/check-docs.mjs
```

相关回归覆盖：大图片持久化与重启恢复、中断和 steering 的完整图片投递、重复请求只投递一次、
63 MiB 精确边界、单 Agent 和全局字节上限、真实 UTF-8 字节计数、投递后容量释放、
既有 32 条队列限制和不确定输入的恢复隔离。

没有替换安装版 daemon 或重启桌面应用；安装版需要包含本修复的后续构建才能生效。
离线原生 fixture 已验证输入传递；真实模型调用、Electron 重打包和 Linux/Windows 未验证。

## 客户端自动压缩与可读分片

在服务端修复提交 `2c452656` 的基础上，发送前统一处理桌面、Web 和移动端图片，
覆盖已有 Agent、新建 Agent、新建 Workspace 与 Workspace setup 的初始输入。
原始附件继续保存并用于预览；发送使用临时编码结果，不覆盖原图。

- 服务端支持的普通小图保留原始字节；AVIF 等可解码但不支持的静态输入格式转为 PNG/WebP。
  需要压缩时，非 JPEG 图片先尝试无损 PNG；随后使用保留透明度
  的 WebP，JPEG 输入沿用 JPEG。质量依次为 0.94、0.88、0.82，达到约 1 MiB 编码目标即停止。
- 超长、超宽以及大型二维图片生成全图概览和有重叠的细节分片。概览最长边为 1024 像素；
  细节分片最长边不超过 2048 像素，保持原始像素分辨率，常规邻接分片重叠 128 像素。
  编码仍超限的区域继续裁成更小的重叠区域，避免无限降低质量或把细节缩成不可读缩略图。
- 分片携带原图编号和裁剪坐标，通过既有 text attachment 提供给模型；发送的 image 对象
  仍只有 `data` 和 `mimeType`。浏览器不支持 WebP 编码时按真实 PNG MIME 返回并继续分片。
- GIF、动画 WebP 和 APNG 保留动画，不转换成单帧。过大的动画明确拒绝。
- 静态 PNG/JPEG/WebP 先读取文件头，超过分片容量或 80 Mi 像素时在解码前拒绝。
  一次只处理一张原图；浏览器 Canvas 只分配概览或分片的尺寸，原生临时文件与引用及时释放。
- 每条消息仍最多 50 张发送图片，base64 图片总量限制为 60 MiB，为文本和封装预留空间。
  处理失败或无法在可读质量下满足上限时，整个发送失败并保留草稿与原图，不静默丢弃附件。

保留服务端接纳修复：SDK 输入仍需与已有校验上限一致，多个可读分片的合计也可能超过旧
4 MiB 上限。没有新增依赖、协议字段或数据库 schema。

编码 API 沿用现有 Expo 14.0.8 的 context/render/save 生命周期，参见
[Expo SDK 54 ImageManipulator](https://docs.expo.dev/versions/v54.0.0/sdk/imagemanipulator/)。

客户端定向验证：81 项单元测试与 4 项真实 Chromium 测试通过；后续调整分片重叠、创建
夹具及输入格式后，18 项直接相关单元测试再次通过。移动端类型检查、变更文件 lint、格式检查与
桌面 Web 资源导出通过。
真实浏览器验证长宽图的最后像素完整保留、细节像素尺寸、透明通道及大 PNG 体积下降。
原生适配器使用离线 mock 验证裁剪顺序、编码选项、引用释放和临时文件删除；未做 iOS/Android 真机验证。

```sh
npm ci
npm run test --workspace=@ait/mobile -- --project unit src/attachments/image-send.test.ts src/attachments/image-send-codec.native.test.ts src/attachments/service.test.ts src/composer/actions.test.ts src/composer/draft/create-flow.test.ts src/composer/draft/create-agent-request.test.ts
npm run test --workspace=@ait/mobile -- --project browser src/attachments/image-send.browser.test.ts
npm run typecheck --workspace=@ait/mobile
node scripts/check-ui.mjs
npm run build:desktop-assets
```

本次补充只改客户端与说明文档，按仓库规定没有重新运行 Rust 测试。

## Test coverage

**Not measured**：用户未要求覆盖率测量；只执行直接相关测试。
没有本次覆盖率百分比、covered/total 行计数、覆盖率产物或可比较基线。
如后续要求测量，使用 `cargo llvm-cov nextest -p provider --lib` 并沿用上述过滤器，
记录对应源码提交、行统计和可审阅产物。测试通过数量不代表覆盖率。
