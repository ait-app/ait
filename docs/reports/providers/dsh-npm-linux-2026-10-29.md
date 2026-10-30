# DSH npm / npx 动态发现验证（2026-10-30 更新）

## 结论

发现逻辑应使用实际安装信息，不能根据反馈中的 `/home/new/.npm-global/bin/dsh` 猜测其他用户的目录。
最终实现优先使用 PATH 中的 CLI；缺失时查询 npm 生效的 prefix 和 cache，
据此发现全局 CLI 和已有的 npx 包。已移除固定 `.npm-global`、默认 `.npm` 根目录兜底，
也移除了手写 `.npmrc` 解析器。npx CLI 的相对入口来自包的 `bin` 声明，不固定为 `lib/bin.js`。

真实包 `@deepseek-ai/dsh@0.2.1-rc.2` 在任意自定义 prefix、自定义缓存和全局 npm 配置下，
均能完成 CLI 版本和模型目录读取。PATH 中已有 CLI 时不会额外调用 npm。

## 最终实现

PATH 中找不到 dsh 时，仅运行一次：

```sh
npm --global config get prefix cache
```

[npm 的多键查询](https://docs.npmjs.com/cli/v11/commands/npm-config/#get)
返回带键名的路径。npm 自己处理用户配置、全局配置、userconfig/globalconfig 覆盖、
环境变量和路径展开；全局模式排除项目 `.npmrc`。
Ait 只请求这两个路径，不请求完整配置，不输出原始 stderr，不下载或安装包。
查询使用现有有界进程执行器，三秒超时后清理进程组；结果最多接受 64 KiB，且必须包含两个绝对路径。

解析后的 prefix 用于查找 Unix 全局 bin，cache 用于发现 `_npx` 中已经安装的包。
保留已有完整桌面安装包的发现行为；npx 缓存作为其后的回退。
缓存扫描最多 256 个目录，manifest 最多 64 KiB，校验包身份及可执行权限，
从 `package.json` 的 `bin.dsh` 或字符串 `bin` 读取相对入口，拒绝绝对入口和父目录跳转。
多个有效包按包目录修改时间和路径稳定选择。

同步 Provider 装配通过独立短期运行时执行这次异步查询，可在已有 Tokio 运行时中安全调用。
npm 缺失、失败或超时时不猜测安装位置；PATH CLI 和完整桌面包仍可使用。

## 本轮真实安装验证

源码基线为 `4e7a1e80` 加本次工作区变更；原 PR 基于 `3aebf9d9`，使用默认 features。
Linux x86*64、Node `v24.22.0`、npm `12.2.1`，真实 DSH 为 `0.2.1-rc.2`。
在 `/tmp/ait-dsh-dynamic-npm-7m2pffr*` 中隔离 HOME、项目、PATH 和 npm 配置。
初始用户配置为：

```ini
prefix=${HOME}/vendor selected install 62
cache=${HOME}/cache selected by npm 19
```

实际运行 `npm install --global --no-audit --no-fund @deepseek-ai/dsh@0.2.1-rc.2`。
测试 PATH 只保留 Node/npm/npx/sh，避免本机已有的 DSH 掩盖问题。

| 场景                                               | 结果                                                 |
| -------------------------------------------------- | ---------------------------------------------------- |
| PATH 显式包含真实 CLI                              | 版本、模型目录通过；npm 调用标记不存在，证明跳过查询 |
| 只有用户 npmrc 指定的任意 prefix/cache             | 两项通过；标记确认实际查询 npm                       |
| 仅全局 npmrc 指定 prefix/cache，项目配置为错误路径 | 两项通过，项目配置未污染结果                         |
| 环境变量覆盖全局 prefix，安装被移到另一个目录      | 两项通过，遵循 npm 自己的优先级                      |
| 仅有 npx 包，缓存位置只配置在全局 npmrc 中         | npx 版本成功，Ait 版本和模型目录两项通过             |

五种配置共 10 项真实安装检查通过。npx 场景同时隔离全局 prefix，
确保不会复用本机的全局 DSH。

负向控制：创建一个未被配置的 `~/.npm-global/bin/dsh`，从 PATH 去掉 npm/dsh。
可用性为 false，符合预期；没有因固定目录中存在文件而自动采用它。

### Ubuntu / npm 9 兼容验证

本轮在 Ubuntu 26.04.1 容器中，以用户 `new`、Node `v22.22.1`、npm `9.2.0`，
PATH 仅为 `/usr/bin:/bin`，验证以下两种不在 PATH 中的真实安装：

- 全局 npmrc 指定随机 prefix `/home/new/random-real-prefix-73`，通过 globalconfig 环境变量选择配置文件。
- 将同一安装移到 `/home/new/.npm-global`，改由用户 npmrc 的 `prefix=${HOME}/.npm-global` 指定。

两种配置的版本、模型目录检查全部通过，共 4 项；使用的测试二进制包含本轮动态查询实现。
因此 npm 9 和 npm 12 的真实查询均已验证。没有运行 Electron 或桌面 GUI。

## Test execution

```sh
cargo test --locked -p provider local::deepseek_harness::launcher:: --lib
```

结果：16 passed、0 failed、2 ignored；其余 1398 项被过滤。
覆盖查询参数、任意目录、已有运行时、查询失败/超时、无效输出、动态 manifest 入口、
缓存选择、CLI 优先级、桌面包及符号链接。
两个默认忽略的真实安装测试在上述五种隔离配置中分别显式执行：

```sh
<provider-test-binary> local::deepseek_harness::launcher::tests::installed_launcher_ --ignored
```

模型发现使用临时 `DSH_HOME` 和测试占位 Key，没有提交 prompt、创建 session 或真实模型调用。
负向控制故意对“应可用”的安装探针断言相反结果，退出 101，由外层验证脚本确认是预期不可用。

provider 构建、rustfmt、Markdown 格式、文档链接及严格 library Clippy 通过。
本机 Rust 1.99.1 的 all-targets Clippy 仍受既有 18 处 `assert_is_empty` 告警阻塞，
只在命令中关闭该已有 lint 后通过；CI 固定 Rust 1.98.1。

## Test coverage

**not measured**：未请求覆盖率测量，没有当前百分比、可比基线或覆盖率产物。
如后续需要，使用 `cargo llvm-cov` 测量 launcher 定向测试；未运行 workspace 测试或覆盖率。

## 范围与限制

必须让 daemon 的启动环境能执行 npm，才能查询未在 PATH 中的 npm 安装。
一次性 npm/npx `--cache` 参数不会自动保留到其他进程，需要相同的持久配置或环境变量。
Node 运行时也需要可用。更改配置、安装或下载后需重启 daemon 重新发现。
未在反馈者原机运行，也未验证桌面 GUI 的环境继承、真实模型调用或工具执行。
操作步骤见 [DeepSeek Harness](../../operations/deepseek-harness.md#npm-安装与识别排查)。
