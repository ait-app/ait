# Provider 录制回放测试

Adapter 测试除了手写假 CLI，还可以回放从真实 CLI 录下的 stdio 帧：测试启动真实 adapter，
`provider_native::replay::StdioPeer` 替代原生程序按录制回答，adapter 发出录制外的帧时测试失败。
格式与规则见 [ADR-298](../decisions/providers/adr-298-provider-stdio-replay-transcripts.md)。
目前只有 Codex 接入，录制文件在 `crates/provider/codex/tests/fixtures/replay/`。

## 运行回放测试

```sh
cargo test -p provider-codex --lib tests::replay
```

回放离线运行，需要 `python3`。失败信息给出第一条不符的帧（原因、录制帧与实际帧），
或某次启动收到的帧数少于录制。

## 录制或刷新

在 adapter 改变发给 CLI 的帧、或要对新版 CLI 复核时重新录制。录制会运行真实 CLI 并产生一次小额模型请求
（`gpt-6-luna`，低推理强度），需要 CLI 已登录：

```sh
AIT_CODEX_REPLAY_RECORD="$(command -v codex)" \
  cargo test -p provider-codex --lib tests::replay::record_text_turn_transcript -- --ignored --exact
```

录制测试只把 `~/.codex/auth.json`（或 `$CODEX_HOME/auth.json`）复制到临时 `CODEX_HOME`，
个人配置、MCP 服务器、插件和钩子不会进入录制；录制在一个临时工作目录中进行，结束后两个临时目录都会删除。
录制会覆盖原文件，首行记录 CLI 版本与日期。

录制时自动替换：工作目录、临时 `CODEX_HOME` 与 Ait 版本写成占位符，本机用户名、主机名与邮箱改成固定值，
`installationId` 写成 `redacted`。提交前仍须逐行复核，确认没有邮箱、账号或组织 ID、令牌、
主目录路径、主机名或机器 ID；新出现的标识字段加入录制测试的 `redact` 列表后重新录制，不手工改写帧。
模型回答若不是 `ok`，按新录制更新回放测试的断言。

## 为其他 adapter 接入

stdio JSON 行协议的 adapter（Pi、ACP 系列）复用同一格式与 `StdioPeer`：在 adapter crate 以 dev 边启用
`provider-native` 的 `test-support`，参照 `crates/provider/codex/src/tests/replay.rs` 写一个共享的场景函数，
一个默认忽略的录制测试和若干回放测试。
