# GitLab Forge 支持与验证

Desktop 0.0.8 对 GitLab origin 调用 `gh pr view`，导致 GitLab 拒绝 GitHub 专用的
`PullRequestForBranch` GraphQL 查询。本次由 Rust server 识别平台并路由到 `glab`，
支持 GitLab.com、自建域名、SSH alias 和多层 group，错误消息保留 GitLab 身份。

实现对照本地 Paseo `30178c4f58b67f8472901356e1484022bd835de0` 的
`forge-resolver.ts`、`forge-cli-command.ts`、`gitlab-service.ts` 与 GitLab pipeline/check
traits，保持 Rust adapter/application/port 边界，未增加依赖。决策见
[ADR-066](../../../decisions/workspace/adr-066-gitlab-forge.md)。

## 功能

- 按当前分支读取 MR；优先打开的 MR，历史已关闭 MR 必须匹配 HEAD SHA。
- issue/MR 搜索、推送分支后创建 MR、直接合并、开启/取消自动合并。
- MR 讨论和行内线程；保留回复、位置及 resolved 信息，准确报告 100 条 discussion 截断。
- pipeline/job/stage 详情，保留 manual、允许失败和 warning 信息；聚合结果以 pipeline 为准。
- 从 MR 创建 worktree：优先 MR ref，回退源分支，保护已有分支并清理临时 ref；跨项目 MR
  保留 setup 授权要求，不错误跟踪目标项目的源分支。原有 GitHub origin/upstream 回退保留。
- 通过 main 既有的 `ServerInfo.features` 发布 `forge-gitlab-v1`，开启现有 Desktop GitLab
  视图与中立 RPC。旧 server 无此标记时不误报 GitLab 能力。

直接合并使用 `--auto-merge=false`，后端检查 MR 是否可合并；自动合并要求活跃 pipeline，
取消使用 `cancel_merge_when_pipeline_succeeds` REST 接口，沿用 Paseo 对 CLI 默认行为的保护。

## 使用和兼容

在运行 server 的 Host 上安装 `glab` 并登录对应域名：

```sh
glab auth login --hostname gitlab.com
# 自建实例：
glab auth login --hostname code.company.example
```

复用 CLI 登录配置，Ait 不另存 token。未知自建域名通过 `gh/glab auth status --hostname`
识别；未登录时返回 `no_remote`，登录后最多等 30 秒失败缓存过期，或重启 server。
云域名以及此前已识别的 host 会显示对应的 CLI 缺失/未登录状态。

旧 Desktop 0.0.8 连接更新后的 server 可避免错误的 GitHub 查询；完整 GitLab 面板需一并
升级 Desktop。本次创建代码 PR，不更新既有安装包。GitHub 专用项目目录/克隆 API 未扩展为
GitLab 目录 API；已在本地的 GitLab 仓库使用现有项目入口。

## 测试执行

自动化使用临时 Git 仓库、本地 bare remote 和隔离的 `gh`/`glab` 子进程替身，不调用用户
账户，也不向实际远端创建或合并 MR。真实 Rust server 的 HTTP/WebSocket 回归验证能力协商、
全部 Forge 操作、MR worktree 创建和 fork setup 阻止状态，断言 GitLab 路径不发 GitHub 查询。
其他重点包括自建 host 探测缓存、SSH alias、登录失效、缺失 CLI、旧 MR SHA、参数原样传递、
API 失败、可选 enrichment、取消自动合并、fork pipeline、讨论截断及分支回退。

| 检查                                                                                             | 结果                              |
| ------------------------------------------------------------------------------------------------ | --------------------------------- |
| `cargo test --workspace`                                                                         | 1,484 passed、0 failed、3 ignored |
| `cargo build --workspace`                                                                        | 通过，无编译警告                  |
| `cargo clippy --workspace --all-targets -- -D warnings`                                          | 通过                              |
| `cargo fmt --all --check`、`git diff --check`                                                    | 通过                              |
| `cargo doc -p filesystem --no-deps`                                                              | 通过，无文档警告                  |
| App 定向单测（下方命令）                                                                         | 6 文件、94 passed                 |
| `node node_modules/@typescript/native-preview/bin/tsgo.js --noEmit -p apps/mobile/tsconfig.json` | 通过                              |
| 修改的 2 个 TS 文件 `oxfmt --check`、`oxlint --deny-warnings`                                    | 通过，0 warning、0 error          |

```sh
npm test --workspace=@ait/mobile -- --project unit \
  src/runtime/rust-daemon/messages.test.ts src/git/forges/index.test.ts \
  src/git/actions-store.test.ts src/git/policy.test.ts \
  src/git/check-presentation.test.ts src/git/pull-request-panel/checks-summary.test.ts
```

前端复用本机已安装依赖；未重新执行干净的 npm 安装。测试数量独立于下方覆盖率记录。

## Test coverage

测量版本：`0b57724a` 加本 PR 的源码，逐文件 SHA-256 记录于
[共享覆盖率 JSON](gitlab-forge-coverage.json)。测量后仅更新文档和该 artifact。
范围为完整 Cargo workspace、默认 features、macOS `aarch64-apple-darwin`；Rust 1.98.1、
cargo-llvm-cov 0.8.4。使用工具默认文件过滤，无自定义排除，默认不插桩 doctest。

```sh
cargo llvm-cov --workspace --html
cargo llvm-cov report --json --summary-only --output-path /tmp/ait-gitlab-coverage-raw.json
cargo llvm-cov report --lcov --output-path /tmp/ait-gitlab-coverage.lcov
```

| 范围                            | 行覆盖率 | 已覆盖 / 总行数 |
| ------------------------------- | -------: | --------------: |
| Rust workspace                  |   91.89% | 41,648 / 45,322 |
| filesystem                      |   89.37% |  9,415 / 10,535 |
| api                             |   95.56% |   1,635 / 1,711 |
| 新增 GitLab adapter 与 resolver |   95.50% |       934 / 978 |

覆盖率运行同样为 1,484 passed、3 ignored。与 main 上[最近同口径报告](../daemon/paseo-server-pr-validation-2026-09-29.md)
的 91.71%（40,580 / 44,250）相比增加 0.18 个百分点；没有重新测量精确基线，故此为历史
比较，不把差值全部归因于本 PR。JSON 同时记录各 crate、改动生产文件的行数与未覆盖行。
未覆盖行号采用 LCOV DA，分母与 LLVM summary 不同，未混合计算。HTML 另生成在本地
`target/llvm-cov/html/index.html`；上述已提交 JSON 是共享审阅产物。

主要缺口包括探测缓存到期/容量淘汰和锁中毒、部分格式错误或缺失字段分支、讨论 403 与
旧行位置回退，以及子进程/文件系统的部分异常分支。需后续故障注入与真实 Host 验证；
不能由高行覆盖率推导所有 CLI 版本及企业部署已经验收。

## 限制

未使用已登录的真实 GitLab/glab 实例，也未做 Electron GUI、Linux/Windows 或用户侧 0.0.8
安装包验收；结果不能代替公司 SSO、代理、非标准端口或 HTTP-only 实例的验证。保留仓库
既有的 3 项在线 Provider ignored 测试。TypeScript 行覆盖率未测量，只执行相关单测、
类型与 lint 检查。后续在已登录 GitLab Host 上验证 MR 面板、流水线及 fork checkout。
