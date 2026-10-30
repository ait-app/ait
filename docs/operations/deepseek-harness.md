# DeepSeek Harness

安装并配置[官方 DeepSeek Harness CLI](https://github.com/deepseek-ai/deepseek-harness)，
确认 `dsh --profile web --no-open --host 127.0.0.1 --port 0` 可启动原生交互 Host。模型路由、凭据和 profile 配置由 Harness 管理。
Ait daemon 注册 `deepseek-harness`；客户端模型选择器显示 **DeepSeek Harness**，
模型和推理等级从本机 Harness 的实际配置目录发现。

默认优先从 PATH 启动 `dsh`。缺失时，通过 PATH 中的 npm 查询实际全局 prefix 和缓存目录，
按 npm 返回的 prefix 查找全局 CLI；不猜测用户的安装目录。
找不到 CLI 时，Ait 自动检查 DSH 桌面安装包：
Linux 会解析 PATH 中 `deepseek-harness` 的真实位置，并检查 `/opt/dsh-desktop-linux-bin`
和 `/opt/deepseek-harness`；macOS 检查 `/Applications` 与 `~/Applications` 下的 DSH 应用。
仅识别包身份为 `@deepseek-ai/dsh-desktop` 且包含 CLI 入口的安装包。
Linux 使用包内 Node；macOS 没有独立 Node 时以 `ELECTRON_RUN_AS_NODE=1` 启动包内运行时。
没有 PATH CLI 或完整桌面包时，Unix 主机继续检查已有的 npx 安装缓存。
这些入口用于模型发现、会话导入/恢复、ACP 和辅助生成，不会打开 DSH 桌面窗口。
缺失或不完整的安装包仍报告不可用；安装或升级 DSH 后需要重启 Ait daemon 重新发现。

独立启动 daemon 时，可以指定 CLI 路径覆盖自动发现（不要指向桌面 GUI 可执行文件）：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_BIN=/absolute/path/to/dsh
```

## npm 安装与识别排查

[官方 README](https://github.com/deepseek-ai/deepseek-harness#run-from-npm) 的运行示例是：

```sh
npx @deepseek-ai/dsh web
```

该命令可以启动 DSH，但不会安装全局 `dsh`。npm 将下载的包放入缓存，并仅为它启动的
进程加入相应的命令路径，见 [npm exec 文档](https://docs.npmjs.com/cli/v11/commands/npm-exec/)。
Unix 主机通过 `npm --global config get prefix cache` 获取实际目录。
npm 自己处理用户配置、全局配置、userconfig/globalconfig 覆盖和环境变量，Ait 不重复解析 `.npmrc`。
没有全局 CLI 或完整桌面包时，从 npm 返回的缓存根目录的 `_npx` 中发现已有的 DSH。
扫描最多 256 个缓存目录，校验包身份，并从包的 `package.json` 的 `bin` 声明读取可执行入口，
有多个有效缓存时选择包目录最近修改的一项；需要固定版本时可使用本文的显式 CLI 路径覆盖。
配置查询最多等待三秒，只读取 prefix 和 cache 两个路径，不读取完整配置输出；失败时不猜测目录。
Ait 直接启动缓存中的 CLI，不执行 npx、不下载依赖，也不连接外部命令已启动的 Web Host。
首次 npx 下载完成后重启 Ait daemon，即可重新发现；原生 Host 不必保持运行。

也可以在运行 Ait daemon 的主机和用户下使用 npm 全局安装 CLI：

```sh
npm install --global @deepseek-ai/dsh
command -v dsh
dsh --version
```

Linux 的 npm 全局命令位于 `{prefix}/bin`，通常是指向
`{prefix}/lib/node_modules/@deepseek-ai/dsh/lib/bin.js` 的符号链接；Ait 支持这种入口。
`npm install` 未加 `--global` 时只安装到当前项目，`npx @deepseek-ai/dsh` 也不等于安装全局
`dsh` 命令。目录规则见 [npm 官方文档](https://docs.npmjs.com/cli/v11/configuring-npm/folders/)。

即使桌面 PATH 不含 npm 全局 bin，只要同一环境中 npm 可执行，Ait 就会根据 npm 返回的实际
prefix 查找 CLI，支持任意自定义安装目录。全局模式的查询忽略项目 `.npmrc`，不执行配置内容。
一次性 `--cache` 参数只影响该次 npm/npx 命令；需要 Ait 使用该位置时，应在 npm 配置或环境中
保留相同设置。所有入口仍需在运行 daemon 的同一用户下可读、可执行。

若终端能找到 `dsh`，Ait 仍显示不可用，确认启动 daemon 的同一环境中 `node` 可执行。
nvm/fnm 等工具配置的 Node 路径需要由启动 Ait 的环境继承。桌面端会读取登录 shell 环境；
先使用 `SHELL`，再查询用户的登录 shell，Linux 最后回退为 bash，macOS 最后回退为 zsh。
独立运行的 daemon 继承启动它的终端或服务环境。安装或修改环境后，重启 Ait daemon。

若命令可见但模型列表加载失败，在终端运行本文开头的 Web Host 命令，确认原生 Host
能启动；`dsh --version` 成功不能证明插件和原生依赖已安装完整。保留 npm 安装时关于
失败或被阻止的安装脚本提示，并从[应用诊断](diagnostics.md)导出故障详情。
如果 Provider 已被识别但模型列表为空，先在 DSH 中完成账号登录或配置模型凭据；
DSH `0.2.1-rc.2` 的全新 profile 在未配置凭据时可返回空模型目录，这与 CLI 路径缺失不同。
排查时提供安装命令、`command -v dsh`、`dsh --version`、`node --version`、
`npm --version` 与 `npm prefix --global` 的输出。
本次 Linux npm 独立安装的验证范围与限制见[排查记录](../reports/providers/dsh-npm-linux-2026-10-29.md)。

## 创建与原生能力

创建示例：

```json
{
  "config": {
    "provider": "deepseek-harness",
    "cwd": "/absolute/workspace/path",
    "title": "Harness task"
  },
  "initialPrompt": "Read this repository and explain its architecture"
}
```

向 `agent.create.request` 发送此 payload。可省略模型沿用 Harness 默认值；
指定模型时使用 `provider.models.list.request` 返回的完整 `id`，不要把 opaque ID 改写成裸模型名。
`thinkingOptionId` 也应使用返回选项的 `id`；某些模型的空字符串表示 provider default。

模式菜单从 DSH 的 `agentPresets/list` 读取当前 profile 的插件情景模式，包括用户自定义预设；不硬编码内置名称。权限单独显示为 Permissions，选项来自原生权限目录，在下一次输入前应用。
工具审批保留一次性允许或拒绝，question 支持单选、多选与自由文本，回答会完成原生待决请求。通过现有
`agent.permission.resolve.request` 回答；`agent.cancel.request` 取消当前工作。
服务重启后 `agent.resume.request` 恢复已登记 handle。原生模式读取展示历史时，从 DSH 的完整记录
恢复用户消息、助手消息和工具时间线；旧版本遗漏的用户气泡会在 daemon 重启后的首次历史加载时修复。
此过程只读，不重新发送用户输入。记录不完整时保留 Ait 现有历史并报告读取失败。
工具参数的空字符串按 DSH 原生规则转为空对象；非法 JSON 原样交给工具校验。

目前支持文本、附件、受 native capability 约束的图片输入、图片输出、工具生命周期、
上下文用量、DSH profile 配置的 MCP、审批、question、权限模式、取消、会话恢复与外部会话导入。
在工作区的导入会话列表选择 DeepSeek Harness，或通过 `provider.sessions.recent.list.request`
列举已有会话；指定 `cwd` 时只显示该目录，未指定时跨目录发现。列表过滤原生标记为空的 probe 和子智能体会话；旧 DSH 未缓存空白状态时，会话可能仍显示在列表中。
`agent.import.request` 只读检查完整历史，保留原生 ID、模型、推理档位和权限配置；
导入不发送 prompt、不创建原生会话。未结束的回合或不完整日志不能作为已完成历史导入。
导入后的 Ait 会话可在 daemon 重启后继续原生对话；DSH 自定义权限组合保持原样，不强制换成内置 preset。
原生历史文件与工作目录分开保存，因此原目录删除后，会话仍可能出现在导入列表中。
导入需要原工作目录存在且可访问；目录不可用时返回 `working_directory_unavailable`，
界面显示原路径并提示恢复目录后重试。重新创建空目录只能恢复导入所需的路径，不能恢复原项目文件。
暂不支持其他前端实时历史同步、steer、rewind、commands 和结构化输出约束。
原生 Host 不接受 Ait 每会话 MCP override；请在 DSH web profile 中配置 MCP。
显示按原生已落盘消息更新，不保证逐 token 输出。
原生模型发现只读目录，不再创建 probe session；显式 ACP 模式仍需打开会话进行发现。

旧 CLI 或需要 ACP 每会话 MCP override 时，可在 daemon 启动前设置：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT=acp
```

ACP 没有权限模式、question 和外部会话导入。默认不会因原生 Host 出错而悄悄回退 ACP；
原生 Host 创建的 handle 不能交给 ACP 恢复。旧 ACP handle 可由原生 Host 接续，并从原生记录恢复展示历史；显式 ACP 模式仍保留原有本地展示历史。

实现边界见 [ADR-082](../decisions/providers/adr-082-deepseek-harness-native-host.md)，
测试范围见[原生 Host 验证报告](https://github.com/KirisameLonnet/ait/blob/dc6cb1e01158ba14120e471b980ffe902ec7e09b/docs/reports/providers/deepseek-harness-native-host.md)。

## 导入列表预览

导入列表保留 DSH 原生标题、目录与活动时间，优先使用 turnOutline 的首尾用户 prompt。旧会话没有缓存摘要时，通过只读历史快照及分页补齐；不会提交消息或创建 Agent。预览规范化空白并限制为 300 个 Unicode 字符。单会话读取最多两秒、8 MiB / 100 页，全列表额外预算十秒；超限、损坏或只有图片而没有用户文本时保留原列表项，不伪造 prompt。

## 模型发现

Provider 模型菜单直接读取原生 Host 的 `session/modelCatalog`，不会为读取目录创建原生会话、选择模型或发送输入。情景模式、默认模式和权限选项也来自只读原生目录，创建/恢复时仍由原生状态校验选择。目录读取不再依赖默认会话能否成功初始化。CLI 启动、原生目录或认证错误仍可能使 Provider 显示错误，应结合展开后的错误文字和 DSH 版本诊断。

## DeepSeek 账号余额

DSH 用量卡片只查询 DeepSeek。内置 DeepSeek API Key 路由已配置时，Ait 从原生 Host 的
`settings/describe` 与 `credentials/describe` 确认凭据引用和实际来源，再调用官方只读
`GET https://api.deepseek.com/user/balance`。显示官方返回的总余额、充值余额、未过期赠金与
API 余额可用状态，保留 CNY/USD 币种；不从余额反推已消费金额、套餐比例或累计 token。
官方总额与分项可能各自舍入，Ait 不重新相加计算总额。

凭据来源遵循 DSH 的启动环境、原生凭据文件、项目 `.env`、DSH home `.env`。
支持自定义 `apiKeyEnv` 引用、`DSH_HOME` 与原生 credentials-local 的静态 `path`/`dshHome`。
会话查询保留该会话启动时的环境与 `.env` 快照，原生凭据文件的更新在后续查询生效；
不能读取某一已选来源时，不换用其他来源的 Key。主机查询使用 daemon 的环境和工作目录，
保留五分钟内存缓存；强制刷新绕过缓存。停止的会话不会改查主机账号。

API Key 未配置时，读取 DSH 的 `account/getBalance`，展示账号登录对应的充值和赠送余额。
二者同时存在时优先展示 API Key 余额，不合并不同认证身份。仅配置 API Key 的用户无需额外登录
DSH 的账号界面。认证、余额为零、缺少接口、网络失败分别处理，原始错误正文和凭据不进入 RPC。

查询不创建原生会话、不发送 prompt、不写入 Ait 凭据存储或账号余额文件。
自定义非官方端点、未知凭据后端、可执行凭据路径配置以及显式 ACP 传输返回明确不可用原因；
不会将这些场景当成已登录账号的零余额。旧版本缺少账号 API 时同样明确提示能力缺失。
本机已验证 DSH `0.2.0-rc.2` 的 API Key 路径；原生账号登录余额使用模拟 Host 验证，详见
[复现与验证报告](../reports/providers/dsh-deepseek-balance-2026-10-14.md)。

## DSH 0.2 权限目录

新版 Host 从 `permissionPresets/catalog` 提供可选权限，历史投影只保留当前值；Ait 兼容此协议及旧版内嵌选项。恢复桌面中打开的原生会话前，应先让 DSH 释放该会话的写入所有权；即使没有运行中的回合，桌面仍可能持有写入锁。只读导入不需要抢占写入所有权。验证范围见[权限目录兼容报告](../reports/providers/dsh-permission-catalog.md)。

## 情景模式、插件与 profile

情景模式是插件组合，决定工具、提示词和技能；它与权限预设相互独立。
Ait 使用 `agent-preset:<原生 ID>` 作为模式 ID，创建请求直接传递原生 `agentPreset`。
模式以目录返回的名称和描述显示，损坏预设不可选。未指定模式时沿用 DSH 默认值。
已有会话保留原生组合；更换情景模式请新建会话。导入会话不修改其插件或权限。

独立权限设置使用 `featureValues.permission_preset`，例如 `"read-only"`，实际可用值以
`provider.features.list.request` 为准。旧 Ait 保存的无前缀权限 `modeId` 仍兼容；
未设置权限时保留原生值，包括自定义权限组合。配置校验不会创建临时探测会话。

默认使用 `web` profile。若插件安装在其他支持 Web Host 的自定义 profile，设置：

```sh
export AIT_SERVER_DEEPSEEK_HARNESS_PROFILE=team-web
```

该 profile 必须已经配置好原生 Web Host。启动参数由 DSH 校验，配置错误不会自动回退。
CLI 不能直接启动 Electron 专属 `desktop` profile；桌面安装包内的 CLI 与桌面 profile
是不同概念。桌面专属插件不会自动复制到 web，请在选定的 Host profile 中通过 DSH 管理插件。
更换 daemon profile 后，已有会话需要原生 ID 对应的插件组合仍可用。

安装或更新插件后刷新 Provider 目录可发现新预设；现有会话仍受原生组合生命周期约束。
插件管理工具若由所选模式提供，会通过 DSH 的普通工具和审批通道运行。
Ait 不托管插件专属 Web 界面或插件安装面板。详见 [ADR-147](../decisions/providers/adr-147-dsh-native-plugin-presets.md)。
