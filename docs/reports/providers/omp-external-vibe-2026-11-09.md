# 外部 OMP Vibe 编排验证（2026-11-09）

功能实现提交：`e6fee4ab04f7c952cbc85a0b70ad21f3b8f021c7`；随后集成 main
`8831b7308ecc9e5fc758abb306870501455a19cf`，重新执行下述 OMP 检查。
平台：Linux x64，Rust 1.98.1，
用户安装的 `/usr/bin/omp` 18.17.0。架构边界见
[ADR-223](../../decisions/providers/adr-223-omp-native-host-bridge.md)，操作见
[Oh My Pi 手册](../../operations/oh-my-pi.md#外部-omp-与-vibe-能力)。

## 执行范围

真实启动外部 OMP 的 RPC 进程，加载 Ait 的公开扩展，模型 HTTP 请求送到隔离的本地
模拟 API。OMP 实际处理工具调用、文件写入、审批和会话日志；没有替换 OMP 二进制，
没有载入 SDK 私有类，没有安装 Bun，没有使用用户认证或付费模型接口。

| 检查                         | 可复现断言                                                                                                                                                         |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 模式开关                     | 模型服务停止时，裸 `/vibe` 仍可进入和退出，确认无需推理                                                                                                            |
| Director → worker → director | `/vibe <指令>` 作为主回合；模型看到五个 Vibe 工具而无 write/bash；spawn 和 wait 经实际 host-tool RPC 执行，worker 实际写出 `OMP_WRITE_OK`，director 收到结果后完成 |
| Worker 审批、追问、恢复      | Ask 模式下文件在批准前不存在；worker 审批由父会话路由；追问保留同一原生 UUID；重启后模式关闭，重新进入后可恢复该 worker 继续追问                                   |
| 会话归属                     | 提交 worker prompt 前核对原生 session header 的 parentSession；恢复时再核对；错误父日志被拒绝                                                                      |
| 取消与 steering              | worker 等待写入批准时接受 steering；取消 director 后 worker 标为 canceled、审批清空、文件未被写入；启动未受理时的任务也由取消路径收回                              |
| 不同模型与 thinking          | 高思考 director 使用非推理 fast 模型仍能完成；worker 采用自身角色的显式 thinking 或原生默认，不带入不兼容的 director 档位                                          |

## 定向检查

```sh
cargo test --locked -p provider --lib local::omp
AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider --lib installed_external_omp_vibe -- --ignored
cargo build --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all --check
npm run check:docs
npx oxlint crates/provider/src/local/omp/vibe/host/extension.ts
```

OMP 定向测试 184 通过、21 忽略；其中 5 项外部 Vibe 实装测试用第二条命令显式执行，
5 项全部通过。其余忽略项属于其他外部能力或真实模型验收，本次未执行。
Workspace 构建、Clippy、格式和文档链接检查通过。未执行完整 Rust workspace 测试。

## 合入 main 的集成检查

保留两侧 Provider 报告索引。Claude 启动函数拆出 advisor 环境配置，保持上游 Linux
配置目录规则和 advisor opt-out 同时生效；`advisor` 过滤测试 6 项、Claude 环境测试
3 项通过。草稿生命周期测试补齐 host、偏好、快照与 toast 依赖模拟，仍运行真实的
草稿 feature hook；草稿与 feature hook 共 16 项通过。测试未降低并行度或放宽超时。

桌面 daemon-manager 10 项、客户端 protocol/messages 45 项通过；重新构建 SDK，
移动端及桌面 typecheck、改动文件 lint/格式和文档检查通过。

## Test coverage

**未测量**：用户未请求覆盖率；以上通过数只表示测试执行结果。如果后续需要，使用
`cargo llvm-cov` 对 OMP 适配器和相关回归定向测量，不默认执行完整 workspace。

## 限制

本报告验证接口和执行链，不验证真实上游模型的规划质量。未执行桌面 UI 可视验收、
正式签名包安装验收或其他 OMP 版本矩阵；此前私有运行时的截图不计入本次证据。
Worker 完整历史保存在原生日志，工具和界面仅传递有界状态与答案预览。
