# Linux npm / npx DSH 识别修复验证

## 结论

Linux 上，原版本能发现 PATH 中的全局 DSH，但遗漏了 PATH 外的用户 npm 全局目录，
以及官方 `npx @deepseek-ai/dsh web` 下载的缓存。
本 PR 同时补齐这两种入口，均已验证 CLI 可用性、版本读取和原生模型目录发现。
验证覆盖 npm 用户 prefix、默认 npm 缓存及环境变量指定的自定义目录。

后续截图进一步确认，反馈者实际有 `/home/new/.npm-global/bin/dsh`，版本 `0.2.1-rc.2`。
此前全局安装实测只覆盖 bin 已在 PATH 中的条件，npx 修复不能解释或覆盖这个额外场景。
补充复现确认：终端通过 bashrc 加入全局 bin 时可运行，启动 PATH 缺少该目录时，
`7d82e367` 中的 launcher 仍报告不可用。现在同时补查 npm prefix 和常见用户全局目录。
这证明该启动环境差异能产生同样现象，尚未读取反馈者原机的 daemon 环境。

| 运行方式                            | 修复前             | 修复后             |
| ----------------------------------- | ------------------ | ------------------ |
| npm 全局安装，bin 在 PATH 中        | 版本及模型目录通过 | 版本及模型目录通过 |
| npm 全局安装，用户 bin 不在 PATH 中 | CLI 不可用         | 版本及模型目录通过 |
| 仅有 npx 缓存，PATH 中没有 dsh      | CLI 不可用         | 版本及模型目录通过 |

官方 [README](https://github.com/deepseek-ai/deepseek-harness#run-from-npm) 的示例是
`npx @deepseek-ai/dsh web`。根据 [npm exec 文档](https://docs.npmjs.com/cli/v11/commands/npm-exec/)，
包下载到缓存，其命令路径只加入 npx 启动的进程。此前 Ait 只检查 PATH 和桌面包，遗漏了这个入口。

## 修复行为

Unix 主机依次选择 PATH CLI、用户 npm 全局 prefix、完整桌面包、已有的 npx 缓存。
全局 prefix 优先读取 `npm_config_prefix` / `NPM_CONFIG_PREFIX`，然后读取用户 `.npmrc` 的
根级 `prefix`，最后补查 `~/.npm-global/bin`。用户配置路径支持
`npm_config_userconfig` / `NPM_CONFIG_USERCONFIG`；支持引号、注释、`~/` 和环境变量展开。
只读至多 64 KiB 的用户配置，不读取项目 `.npmrc`，不通过 shell 执行配置值。
缓存来自 `npm_config_cache`、`NPM_CONFIG_CACHE` 或默认 `~/.npm`，路径必须为绝对路径。
仅在 `.npmrc` 或某次命令参数中配置的自定义缓存，需要额外导出环境变量。
扫描 `_npx` 下至多 256 个目录；只接受包名为 `@deepseek-ai/dsh`、
`bin.dsh` 为 `lib/bin.js` 且入口可执行的安装。manifest 最多 64 KiB。
多个有效包按包目录修改时间选择，时间相同时按路径确定；显式 CLI 覆盖仍可固定版本。
发现过程不运行 npm/npx、不下载、不修改缓存；使用独立的原生 Host 读取目录。

## 实测环境与隔离

Linux x86_64、Node `v24.22.0`、npm `12.2.1`、Rust `1.99.1`，
真实包版本为 `@deepseek-ai/dsh@0.2.1-rc.2`。
源码基于 `3aebf9d9` 加本次工作区变更，默认 features。
初始失败探测基于此前的 `50ac8f62`；两基线间没有 provider 变更。

临时目录 `/tmp/ait-dsh-install-matrix-d35u2cxm` 中分别设置 project、HOME、npm prefix 和缓存。
测试 PATH 仅含 Node、npm、npx、sh 的符号链接；npm 场景额外加入测试 prefix 的 bin。
本机 `/usr/bin/dsh` 不在 PATH 中，`/opt` 下也不存在可用桌面包，避免其他安装掩盖失败。

实际执行了 `npx --yes @deepseek-ai/dsh@0.2.1-rc.2 --version`，版本读取成功；
同一环境的修复前 Ait launcher 随后在 `is_available()` 断言失败。
接着实际执行 `npm install --global --no-audit --no-fund @deepseek-ai/dsh@0.2.1-rc.2`，
将测试 prefix/bin 加入 PATH 后，版本和模型发现均通过。

修复后，对相同真实安装分别运行：全局 bin 在 PATH、自定义 `npm_config_cache`、
自定义 `NPM_CONFIG_CACHE`、默认 HOME/.npm。后三种环境的 PATH 均没有 dsh。
默认缓存场景将同一下载缓存移到 HOME/.npm，并通过离线 npx 版本命令确认可用。
四种环境的两项原生验证全部通过，共 8 次通过。

### 自定义 prefix 与 Ubuntu 26.04 补充验证

在 `/tmp/ait-dsh-global-prefix-0a2uwl1a` 创建隔离 HOME、仅含 Node/npm/sh 的 PATH，
以 `prefix=${HOME}/.npm-global` 安装真实 `0.2.1-rc.2` 包。
bash 终端加载测试 bashrc 后，`command -v dsh` 和版本命令成功；
不加载 bashrc 的启动环境找不到命令，修复前的 launcher 可用性检查失败。

补充修复后，下列六种不含全局 bin 的 PATH 环境，各自的版本和模型目录检查均通过：
常见 `.npm-global` 加 npmrc、常见目录但无 npmrc、npmrc 指定带空格的其他 prefix、
小写 prefix 环境变量、大写 prefix 环境变量、自定义 userconfig 文件。共 12 次通过。

另外在 Ubuntu **26.04.1 LTS** 容器内，以用户 `new`、HOME `/home/new`、
Node `v22.22.1`、npm `9.2.0`，使用相同 npm 安装布局。
PATH 仅为 `/usr/bin:/bin`，其中没有 dsh，真实入口为
`/home/new/.npm-global/bin/dsh`。修复后的可用性/版本和模型目录两项检查通过。
该验证覆盖 Ubuntu 容器运行环境，没有运行桌面 GUI 或 Electron。

补充修改后再次在独立 HOME 下执行真实 npx，并重新验证缓存入口及模型目录，两项通过。
该环境同时隔离 PATH 和 npm 全局 prefix，确保 npx 不会复用本机已有的全局 DSH。

桌面代码并未固定 Linux 使用 zsh：选择顺序是 `SHELL`、用户登录 shell、Linux 回退 bash。
本次没有修改 shell 选择行为，也未从截图推断反馈者的实际 shell。

## Test execution

```sh
cargo test --locked -p provider local::deepseek_harness::launcher:: --lib
```

结果：17 passed、0 failed、2 ignored；其余 1398 项被过滤。
覆盖 npm 符号链接、缓存入口运行、配置优先级、PATH/桌面优先级、坏 manifest、
错误包身份、缺失或不可执行的入口、较新缓存选择与时间相同的稳定选择，
以及全局 prefix 配置优先级、用户配置覆盖、路径展开、坏配置与桌面 PATH 缺失。
最终 HOME 路径校验调整后，额外定向复核 npm_prefix 模块的 7 项测试：

```sh
cargo test --locked -p provider local::deepseek_harness::launcher::npm_prefix:: --lib
```

两个默认忽略的真实安装验证，在上述四种隔离环境中显式启用：

```sh
<provider-test-binary> local::deepseek_harness::launcher::tests::installed_launcher_ --ignored
```

每种环境均为 2 passed、0 failed；每次模型发现都使用临时 `DSH_HOME` 和非真实 Key 占位值，
使原生目录包含模型。没有提交 prompt、创建 session 或验证真实模型服务认证。
未配置凭据的全新 profile 可以返回空目录，这与 CLI 未发现是两个问题。

`cargo build --locked -p provider`、`cargo fmt --all --check`、`git diff --check`、
Markdown 格式检查和 `node scripts/check-docs.mjs` 通过。
`cargo clippy --locked -p provider --lib -- -D warnings` 通过。
本机 Rust 1.99.1 下，严格 all-targets Clippy 受既有测试的 18 处
`clippy::assert_is_empty` 阻塞；CI 固定 Rust 1.98.1。
`cargo clippy --locked -p provider --all-targets -- -D warnings -A clippy::assert_is_empty`
通过，仅在命令中放宽这个已有 lint，没有修改源码 lint 配置。

## Test coverage

**not measured**：未请求覆盖率测量，没有当前覆盖率百分比、可比基线或覆盖率产物。
如后续需要，使用 `cargo llvm-cov` 测量 launcher 定向测试。
没有运行完整 workspace 测试或 workspace 覆盖率。

## 限制

本次验证入口和模型发现，未测试真实模型调用、工具执行或桌面 GUI 的环境继承。
npm 12 提示有五个依赖的安装脚本被阻止，但未影响本次版本和目录验证。
Ubuntu 26.04.1 容器验证通过；未在 macOS、Windows 或反馈者机器上运行。
清理 npx 缓存后需重新下载，
安装、下载或配置变更后需重启 daemon 重新发现。
操作步骤见 [DeepSeek Harness](../../operations/deepseek-harness.md#npm-安装与识别排查)。
