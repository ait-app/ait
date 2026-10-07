# Filesystem 移除 metadata 依赖的验证

日期：2026-10-07。对应 [ADR-103](../../decisions/workspace/adr-103-filesystem-model-collaboration.md)。
源码是 `612a816f99cbe29797b795fccb64dc64bf612afc` 加本报告所在提交的迁移；[验证证据与 Rust 源码摘要](filesystem-model-independence-2026-10-07.json)
保存逐文件 SHA-256、测试命令和结果，区分本轮修改与提交前的 model/provider/terminal 快照。

## 验证结果

提交前 `cargo test --workspace`：1938 通过、0 失败、10 忽略。覆盖率运行的测试结果相同。
构建、Clippy、格式与文档检查再次通过。以下保留本地迭代的定向结果。

577 项通过、0 失败、0 忽略。范围为 model runtime/Workspace、filesystem crate、metadata 目录/命名、
API 组装/分发、daemon 宿主与依赖守卫，以及 clone/worktree/setup/checkout 的 daemon 进程测试。
macOS arm64、默认 features；其他模块按下面过滤器排除，没有额外跳过所选范围中的测试。

```sh
cargo test -p model --lib -- runtime:: workspace:: --test-threads=1
cargo test -p filesystem --lib -- --test-threads=1
cargo test -p metadata --lib -- service::directory:: service::workspace_names:: --test-threads=1
cargo test -p api --lib -- composition:: connection::dispatch:: --test-threads=1
cargo test -p daemon --bin daemon -- host::tests:: --test-threads=1
cargo test -p daemon --test dependencies -- --test-threads=1
cargo test -p daemon --test process -- github_projects:: worktrees:: workspace_automation:: agent_execution::worktrees:: checkout:: --test-threads=1
```

`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all --check`、
`git diff --check` 和 `npm run check:docs` 均通过。
`cargo metadata --format-version 1 --no-deps --locked --offline` 确认 filesystem 的唯一内部依赖是 model。
filesystem 的 149 个 Rust 源文件包含测试，均没有 `metadata::` 导入；没有添加 metadata 开发依赖。

GitHub clone 的完成路径和时间通过登记接口传递，失败保留 checkout；
Project 登记适配器复用原有记录，命名与 setup 复用已有服务，Runtime 的共享入口覆盖
预算耗尽、取消、业务错误、panic 和响应丢弃后的任务跟踪。
初次新增登记测试误把不发布通知的 Registry 替身当成通知实现；已将断言限定到共享记录和安全错误，
最终结果全部通过。

## Test coverage

提交前命令：`cargo llvm-cov --workspace --html`。测量整个 Rust workspace、默认 features、
macOS arm64，没有额外文件排除；10 个安装/认证 Provider 用例按默认设置忽略，Windows/Linux 未测量。
HTML 位于 `target/llvm-cov/html/index.html`；可评审的逐文件覆盖率与 SHA-256 保存在
[验证证据](filesystem-model-independence-2026-10-07.json)。

| 范围                | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------- | -------: | --------------: |
| `workspace`         |   94.45% |   54897 / 58122 |
| `crates/filesystem` |   94.90% |   11567 / 12189 |
| `crates/model`      |   96.37% |     2495 / 2589 |
| `crates/metadata`   |   94.29% |     6089 / 6458 |
| `crates/api`        |   92.74% |     2005 / 2162 |
| `bins/daemon`       |   95.52% |       939 / 983 |

与相同平台和命令口径下的 `612a816f` 相比，workspace 从 94.43%（54841/58076）变为
94.45%（54897/58122），增加约 0.02 个百分点；定义迁移会改变各 crate 的分母。
[前一提交的报告](../daemon/model-capability-boundaries-2026-10-07.md)仍对应原来的源码。

共享身份函数 112/112 行、命名输入 7/7 行、descriptor 投影 88/88 行、metadata 协作适配器
44/44 行和 API 组装适配器 63/63 行均被覆盖。Runtime 为 92/95 行，剩余集中在排队执行时的
admission 取消竞态分支；后续修改 admission 行为时需补对应竞态测试。
真实远端 GitHub、需要认证的原生 Provider 与其他平台未在本轮验证。
