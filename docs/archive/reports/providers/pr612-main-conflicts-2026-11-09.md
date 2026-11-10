# PR #612：合入 main 与能力目录验证

基线为 feature `307f8259` 与 main `f8e8cea4`（#628、#629）。解决七个测试文件的
合并冲突，保留双方方法与测试；本报告与合并提交一同交付。Linux x64，Rust 1.98.1。

## 能力目录

生产组件实际声明 236 个方法，发布能力 237 项（含 `connection.single.v1`），
业务目录及空宿主占位方法 217 项。客户端方法表 225 项，四个通道的 hello 能力数量为
64 / 62 / 44 / 62，均不超过 daemon 的 64 项限制。

API 所有权测试同时核对预设、原生命令和飞书、Telegram、微信、QQ 的新增入口。
daemon 进程测试从真实 HTTP ServerInfo 验证完整注册和实现目录，没有占位方法遗漏。

## 回执校验修复

隔离 daemon 的真实保存 RPC 成功写入后，生成校验器却拒绝其回执。原因是
save/delete 共用 schema 的 discriminator 使用 `z.union`，当前 zod-aot 仅提取 literal
的值。改成 Zod 4 的多值 literal，保持线上的消息类型和载荷不变。四个新增探针覆盖
保存/删除成功与只读拒绝回执；修复前全部失败，修复后通过。

编辑器同时补上 SelectField 的已选模型与思考档位显示，避免实际值存在时仍显示
占位文案。模型与档位变化后的可访问名称已有断言，相关 17 项测试重新执行通过。

## 当前界面取证

本次截图来自当前源码的浏览器界面，连接独立临时数据目录的真实 daemon。OMP 为
用户安装的官方 18.17.0，模型目录使用本地预览配置，地址指向不可用的回环端口；
未发送模型请求。`/vibe` 的裸命令成功启用 director，取证只证明能力握手与模式切换，
worker 推理路径的实装证据仍见此前外部 OMP 报告。

- [模型选择器齿轮](../../../assets/providers/omp/current-model-picker-gear.png)
- [Claude 的统一预设设置](../../../assets/providers/omp/current-provider-presets-settings.png)
- [OMP 项目覆盖与只读权限](../../../assets/providers/omp/current-omp-project-presets.png)
- [模型、思考档位和角色编辑器](../../../assets/providers/omp/current-preset-editor.png)
- [外部 OMP 启用 Vibe](../../../assets/providers/omp/current-external-vibe-enabled.png)

隔离 daemon 的真实保存、删除、重新保存往返均成功通过客户端生成校验器；浏览器
点击保存后编辑器正常关闭。OMP 使用隔离配置，没有使用私有 OMP/Bun 副本。

## 定向验证

- API capabilities 9 项、connection validation 3 项、single lanes 2 项、placeholder
  筛选 4 项、HTTP readiness 1 项通过（placeholder 筛选包含一次重复的 validation）。
- daemon 组件安装与真实进程 catalog 各 1 项通过。
- 客户端 connection/messages：99 项通过；协议 Bot/预设回执：12 项通过。
- 前端 Bot channels、预设编辑器、翻译：89 项通过。
- SDK 构建、桌面/移动端 typecheck、Rust workspace 构建和 Clippy、rustfmt、oxlint、
  仓库 oxfmt、UI 检查、文档链接检查通过；客户端/Rust 方法声明校验与解析器 3 项通过。

未在本地运行完整 Rust workspace 测试。PR CI 的执行结果另见 GitHub 当前 head。
截图的可视取证范围另在 PR 对应功能小节注明，截图不代表模型推理或正式安装包验收。

## CI 协议兼容检查

`272386c1` 的 UI CI 暴露旧 rename 测试辅助函数仍读取 literal 的单个 `.value`，
遇到新增的多值 literal 会抛错。辅助函数改为检查 `.values`，并验证移除 rename 消息
后仍保留预设 save/delete 回执。相关 rename 与预设 8 项测试、协议 typecheck、
oxlint、oxfmt 通过。该后续修复只改变测试辅助代码，不改变线上消息或运行行为。

## PR 说明与 main 差异核对

审查快照为 `f8e8cea4..272386c1`：392 文件，新增 30,463 行、删除 820 行；其中
生产实现与接线 168 文件、15,019 行增删，测试/夹具 135 文件、13,113 行增删，
文档 29 文件、2,183 行增删，翻译 9 文件、962 行增删，构建配置 2 文件、6 行增删，
截图 49 文件（不计二进制行数）。按生产 Rust/TS/JS 物理行统计，main 基线为
471,019 行，变动量占 3.19%；不是重写比例，也不是测试覆盖率。

原 15 个功能小节覆盖主要 OMP/UI 行为，但对 DSH 原生命令、附件上传、ZIP 导出、
无激活历史读取，及 OpenCode ACP 命令目录与 2.1.3 settlement 门控解释不足。
两组共 24 个生产文件、1,709 行增删，占生产变动量 11.38%；这表示需要补充分组
说明，不表示这些行全部未被提及。跨 provider 进程组回收、桌面 Provider 环境保留、
Babel 排除协议文件的编译优化及 package:daemon 入口也应单独交代。

## Test coverage

**未测量**：用户未请求覆盖率。若后续请求，使用 `cargo llvm-cov` 对能力目录与回执
校验涉及的范围定向测量；本报告中的通过数量不是覆盖率。
