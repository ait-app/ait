# PR #612：预设来源与 Vibe worker thinking 复审修复

源码：与本报告一同提交的复审修复，基线 `8e5de5c297cc92b8b5dcf936d8db693b7e5a1cc8`。
平台：Linux x64，Rust 1.98.1，用户安装的外部 OMP 18.17.0。

## 修复与行为证据

预设列表将配置、来源和可编辑状态放在同一个响应中；这些字段只用于读投影，
不会写进 daemon 的预设存储。旧 daemon 缺少元数据时不授予修改权限。来源未知、
刷新中或查询失败时禁用编辑、删除和已打开编辑器的保存。提交前重新读取列表，
将新来源或失败状态同步回共享缓存；项目覆盖变为只读后不能提交旧表单。

后端在命令探针启动前、执行原生 save/delete 前都读取项目作用域。存在同名项目覆盖、
来源读取失败，或覆盖在探测期间出现时，保存与删除均在写入前拒绝。
测试逐字比较全局配置文件，确认原全局同名条目保持不变；项目配置也没有被改动。

Vibe worker 移除父配置的隐藏 `modelPresetThinking` 后，再应用自身角色的显式后缀。
新增实装用例读取真正的命名父预设（low）和原生默认（high），通过公开扩展 API 观察
worker 的实际 thinking：无角色后缀为 high，显式 low 后缀仍为 low。模型 HTTP 仅访问
隔离的回环模拟服务，不使用用户认证或付费模型。

两个修复都做过负向验证：回退前端修复后 5 个新增来源探针失败；仅回退 worker 的隐藏
字段清理后，实装探针读到 `Native worker thinking: low`，与期望 high 不符。
负向验证后恢复修复，再完成正向检查。

## 定向验证

```sh
cargo test --locked -p provider --lib preset
cargo test --locked -p provider --lib local::omp::vibe::host
AIT_TEST_OMP_BIN=/usr/bin/omp cargo test --locked -p provider --lib installed_external_omp_vibe -- --ignored
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked --workspace
cargo fmt --all --check
npm --prefix apps/mobile run test -- --project unit src/components/provider-presets-section.test.tsx src/i18n/resources.test.ts src/components/provider-diagnostic-models.test.ts
npm --prefix packages/protocol run test -- src/messages.provider-presets.test.ts
npm --prefix apps/mobile run typecheck
npm run check:docs
```

预设相关 Rust 测试 46 通过、2 忽略；Vibe coordinator 单元测试 6 通过；外部 OMP Vibe
实装检查显式执行 6 项，全部通过。前端 3 个 suite / 59 项通过；协议及生成校验器
2 项通过。SDK 构建、workspace 构建/Clippy、移动端类型检查和改动文件格式/lint 通过。

未执行完整 Rust workspace 测试、覆盖率、真实付费模型推理或正式安装包验收。
此前 head 的 CI 和 184 项回归不计入本次定向执行结果。

## Test coverage

**未测量**：用户未请求覆盖率。通过数是执行结果；若后续请求覆盖率，使用
`cargo llvm-cov` 对本次预设与 Vibe worker 配置范围定向测量。
