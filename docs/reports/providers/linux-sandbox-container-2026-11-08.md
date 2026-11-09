# Linux 沙箱容器验证与 PR review 修复

日期：2026-11-08。对应 [PR #622](https://github.com/ait-app/ait/pull/622)。
首次测试源码为 `1ee000778e83764a29306aaf147686bb41271688`，Linux x86_64、默认 features。后续复查单独记录在文末，不能混用两轮二进制与测试数量。
被测 Provider 测试二进制 SHA-256：`37429f8e89ca98a2b6801cdefa8f9b1e30e3dc29243d94161cbf4e49c8c3b3f3`。

## 结果

Ubuntu 24.04 和 Debian 13 容器均以 UID/GID 1001 运行真实 bubblewrap，`sandbox` 筛选的 34 项测试各自全部通过，没有忽略项。25 个文件系统、子进程和 namespace 越界探针被拒绝，3 个允许操作的对照成功。另行验证发现事先存在的跨边界硬链接仍可修改共享 inode，详见下文；不能把通过的探针解释为所有文件布局都能完整隔离。

| 环境         | bubblewrap | 测试筛选                                     | 结果                    |
| ------------ | ---------- | -------------------------------------------- | ----------------------- |
| Ubuntu 24.04 | 0.9.0      | `sandbox`                                    | 34 通过，0 失败，0 忽略 |
| Debian 13    | 0.12.0     | `sandbox`                                    | 34 通过，0 失败，0 忽略 |
| Debian 13    | 0.12.0     | `local::pi::transport::tests::shutdown`      | 13 通过                 |
| Debian 13    | 0.12.0     | `local::claude::tests::process::environment` | 3 通过                  |

这些筛选存在重叠，不能累加为不同测试数量。Debian 首次运行因测试工具 `/bin/kill` 缺失而有 3 项失败；补装 `procps` 后重跑，获得表中结果，未修改产品代码来绕过该失败。

## Review 对应修复

| 评论                                                                                 | 修复与回归证据                                                                                                                             |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| [Pi 退出清理](https://github.com/ait-app/ait/pull/622#discussion_r4506428833)        | 正常退出只向 namespace 内原生进程发送 SIGTERM，保留 monitor/reaper；4 项受限会话测试覆盖清理、超时强杀、取消后重试和 runtime 结束后 Drop。 |
| [控制终端注入](https://github.com/ait-app/ait/pull/622#discussion_r4506428848)       | 使用 `--new-session`；PTY 测试先创建真实控制终端，确认移除该选项时仍能打开 `/dev/tty`，加入后无法打开。                                    |
| [受保护文件硬链接](https://github.com/ait-app/ait/pull/622#discussion_r4506428862)   | 策略直接保护的普通文件 `nlink > 1` 时拒绝启动，测试确认原内容不被修改。                                                                    |
| [配置初始化竞争](https://github.com/ait-app/ait/pull/622#discussion_r4506428870)     | 私有临时文件写入、同步后 `persist_noclobber` 发布；8 个并发首次初始化者均读取完整 1 MiB 内容，未留下临时文件。                             |
| [HOME-only 配置迁移](https://github.com/ait-app/ait/pull/622#discussion_r4506428881) | 区分显式配置目录与 HOME 推导目录；离线 Claude 协议 fixture 确认已有全局设置被迁移，显式目录和普通会话行为仍通过。                          |

## 隔离探针范围

使用测试自己创建的 home、worktree、linked Git 元数据和假认证文件，不读取真实账号或 key。拒绝测试中的文件在进入沙箱前由同一 UID 成功创建；工作区位于 `/lab`，不处于允许写入的共享 `/tmp`。

- 拒绝 home/兄弟目录写入、覆盖、追加、截断、删除、重命名、chmod 和新建跨挂载硬链接。
- 拒绝 `..`、符号链接、`/proc/self/root`、重新打开 `/proc/self/fd` 的路径绕过。
- 拒绝 Provider 配置、公共/私有 Git 配置、hooks 和 worktree `.git` 指针写入，以及删除/重命名配置、重命名 Git 受保护路径的祖先。
- 拒绝子 shell 越界写入、嵌套 user namespace、把根挂载重新改成可写。
- 允许读取外部文件、工作区内写入/重命名、读取并原子更新 fixture 认证文件。
- 其他沙箱测试验证真实 Git commit、环境变量、临时目录、loopback 网络和 Claude 全局设置原子替换。

文件操作只接受预期权限或挂载错误，错误类型不匹配会失败；子命令探针要求非零退出。测试完成时再次检查外部 fixture 内容保持原状。

## 失败条件对照

默认 Docker 安全配置下，bubblewrap 报告没有创建 namespace 的权限；目标写文件命令未执行，未产生标记文件。实际隔离测试需要允许容器内创建嵌套 namespace 和挂载 `/proc`，因此仅测试容器使用下面的三项安全选项，没有使用 `--privileged` 或增加 `CAP_SYS_ADMIN`。

另在一次性 Debian 容器中以 root 将 `/usr/bin/bwrap` 改名，再降为 UID 1001 运行单项文件系统测试。测试按预期以 101 退出，错误为 `sandbox command: Unavailable`，发生在创建目标命令之前。此项是故障注入的预期失败，未计入表中的通过数量。

## 复现

构建测试程序：

```sh
CARGO_INCREMENTAL=0 cargo test --locked -p provider --lib --no-run
```

容器 Dockerfile：

```dockerfile
ARG BASE=ubuntu:24.04
FROM ${BASE}
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends bubblewrap git python3 util-linux procps libgcc-s1 ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd --uid 1001 --create-home sandbox-test && mkdir /lab && chown sandbox-test:sandbox-test /lab
USER 1001:1001
WORKDIR /lab
ENV HOME=/home/sandbox-test
```

使用该文件构建 `ait-sandbox-test:ubuntu24` 和 `ait-sandbox-test:debian13`，后者传入 `--build-arg BASE=debian:13-slim`。实际 Ubuntu 测试镜像安装列表未显式包含 `procps`，已有 `/bin/kill`；上面的共同配方包含 Debian 所需的补充工具。实际镜像 ID：

- Ubuntu：`sha256:a134c027b13b23765e1e6acd28eebc94de3de2625ca7c24af9772cb775cdef68`
- Debian：`sha256:4c721a08297e59ebdc6fff83667c7edf0895e5ca3204c62a5838e239852a4f29`

只读挂载测试二进制和 fixtures；fixture 目标路径保留编译时的绝对路径，因为测试使用 `CARGO_MANIFEST_DIR`。二进制文件名以当前构建输出为准。

```sh
repo_root=$(pwd)
test_binary="$repo_root/target/debug/deps/provider-719c6224f742e4ef"
docker run --rm --network none \
  --security-opt seccomp=unconfined \
  --security-opt apparmor=unconfined \
  --security-opt systempaths=unconfined \
  --mount "type=bind,src=$test_binary,dst=/opt/ait-provider-tests,readonly" \
  --mount "type=bind,src=$repo_root/crates/provider/tests/fixtures,dst=$repo_root/crates/provider/tests/fixtures,readonly" \
  ait-sandbox-test:debian13 \
  sh -c 'id; bwrap --version; exec /opt/ait-provider-tests sandbox --nocapture'
```

把镜像换成 Ubuntu 可执行同一筛选；把 `sandbox` 换成表中两个模块路径可运行相关回归测试。容器没有挂载宿主凭据、Docker socket 或可写源码目录，运行期间禁用外网；网络探针仅使用容器自己的 loopback。

## 已确认的剩余限制

用相同 bubblewrap 挂载机制额外运行以下对照：进入沙箱前令 `/lab/worktree/alias` 与 `/lab/outside/data` 硬链接，随后只放行 worktree 写入。

```sh
mkdir /lab/worktree /lab/outside
printf original > /lab/outside/data
ln /lab/outside/data /lab/worktree/alias
bwrap --die-with-parent --new-session --unshare-user --unshare-pid \
  --unshare-ipc --disable-userns --cap-drop ALL --ro-bind / / \
  --proc /proc --dev /dev --bind /lab/worktree /lab/worktree -- \
  sh -c 'printf changed > /lab/worktree/alias'
cat /lab/outside/data
```

输出为 `changed`，确认只读外部路径不能阻止经已有可写别名修改共享 inode。此次修复拒绝策略直接保护的多链接普通文件，但未扫描整个可写树或受保护目录后代，也没有防御其他宿主进程在检查后改变文件布局。这是当前未解决的边界限制。更强保证需要单独设计 inode 级约束或不共享 inode 的工作区，不能用这次测试结果替代。

本次没有真实模型调用、完整安装包构建或 macOS/Windows 实测；bubblewrap 最低声明版本 0.8.0 未在本轮容器中执行。保留读取、网络、共享 Provider 状态和宿主服务能力的设计限制见 [ADR-223](../../decisions/workspace/adr-223-linux-worktree-sandbox.md)。

## 构建与静态检查

- `cargo fmt --all --check`。
- `CARGO_INCREMENTAL=0 cargo clippy -p provider --all-targets -- -D warnings`。
- `CARGO_INCREMENTAL=0 cargo build --locked --workspace`，这是构建，没有运行全 workspace 测试。
- 改动 Markdown 格式、Python fixture 语法、`npm run check:docs` 和 `git diff --check`。

## Test coverage

**Not measured**：用户要求容器行为测试和复查，未要求行覆盖率测量；没有生成覆盖率百分比或覆盖率附件。执行结果只适用于各轮标明的源码及定向回归范围。若后续要求测量，使用 `cargo llvm-cov` 对这些定向筛选生成可审阅报告，不扩展为全工作区测试或覆盖率。

## 第二轮 review：保护 commondir

复查基线 `1ee000778e83764a29306aaf147686bb41271688` 的远端 CI 全部通过，但新增探针仍发现 P1 级策略绕过：私有 Git 目录的 `commondir` 文件可写，后续启动会重新解析该文件并放行它指定的公共目录。把指针改成外部 home 可以扩大下一次启动的可写范围。

先以基线二进制运行新增探针，得到 `git-common-pointer: allowed`，测试失败。随后在共同 domain 策略中保护 `commondir`；Linux 对不存在的指针初始化为 `.\n`，保持原有 common-dir 语义；macOS 沿用受保护路径的拒绝规则。已有 linked worktree 指针不会被覆盖。

修复源码为 `1ee000778e83764a29306aaf147686bb41271688` 加本节同提交的变更。新二进制 SHA-256：`f24be3d1de6bf042ced37bdf9d9f4058c73c935deca2d744e70f34a909184f05`。使用前述相同容器和运行命令：

- Ubuntu 24.04 与 Debian 13 的 `sandbox` 筛选各 35 项通过，无失败或忽略；攻击探针增至 28 项，新增覆写、删除、重命名 `commondir`，并在攻击后重新解析策略，确认未放行 home。
- 新增测试验证缺失 `commondir` 初始化后，真实 `git rev-parse --git-common-dir` 仍解析到原目录；linked worktree Git commit 回归仍通过。
- `CARGO_INCREMENTAL=0 cargo test --locked -p domain workspace::sandbox::tests --lib`：7 项通过。
- `CARGO_INCREMENTAL=0 cargo clippy --locked -p domain -p provider --all-targets -- -D warnings`、`CARGO_INCREMENTAL=0 cargo build --locked --workspace`、格式、文档链接与 diff 检查通过。

没有运行全 workspace 测试或行覆盖率测量；macOS 渲染回归断言已更新，但本机没有执行 macOS 测试。原有跨边界硬链接限制仍然存在；共同 Git 目录内其他 worktree 元数据可写等 ADR-215 已声明的共享范围也未在此次修复中收窄。
