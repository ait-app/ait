# ADR-072：Workspace 名称与当前文档边界

- 状态：Accepted
- 日期：2026-10-03
- 授权：统一 daemon 名称、移除 crate 的 server 前缀、重命名应用目录并清理旧文档。
- 修订：此前 ADR 中的源码位置与包名统一遵循本决策；既有能力行为与依赖方向保持不变。
- 后续：crate 集合由 [ADR-111](adr-111-domain-values-and-server-protocol.md)（移除 protocol）与
  [ADR-112](adr-112-persistence-crate.md)（file 改名 persistence）修订，并新增 relay；
  当前 crate 列表以[当前架构](../../architecture/README.md)为准。

## 背景

服务入口和能力 crate 使用 server 名称，桌面与共享界面沿用 Paseo 导入目录。
文档索引同时描述已删除的 CLI、worker 和旧 Project 模型，使当前开发入口和架构难以查找。

## 决策

1. `bins/daemon` 的 Cargo package 与 binary 均为 `daemon`。11 个能力 crate 使用
   `api`、`browser`、`domain`、`filesystem`、`metadata`、`model`、`protocol`、`provider`、
   `schedule`、`terminal`、`voice`，目录与包名一致。
2. Electron 位于 `apps/desktop`；共享 Expo 界面位于 `apps/mobile`，npm 包名为
   `@ait/desktop`、`@ait/mobile`。共享构建入口为 `build:ui-deps`，独立开发入口为 `dev:mobile`。
3. 启动器、测试、CI、安装包与签名资源使用 daemon 二进制。安装包只包含
   `resources/bin/daemon`。发布工具从检出的源码识别布局，允许重建改名前的不可变标签。
4. `AIT_SERVER_*` 配置、已有默认数据目录、身份文件和公开 wire 名称保持兼容。
   本次不删除、移动或迁移用户数据，不引入二进制旧名称别名。
5. 依赖守卫检查全部 workspace 包，不再根据名称前缀筛选。现有能力归属及向内依赖保持不变。
6. 文档按当前架构、分类 ADR、运维、规范、计划和验证报告组织。删除已移除实现的旧 ADR、
   CLI 工作流、旧运维手册和无关验证资料；仍与当前实现相关的决策、报告和第三方许可证保留。

## 后果与验证

本地脚本与外部源码引用需使用新目录和包名。用户现有数据、连接地址、认证变量和协议字段
不因改名改变。旧源码和删除的文档可通过 Git 历史查阅；保留的历史报告只证明注明版本的行为。

当前依赖图见 [架构说明](../../architecture/README.md)，测试、构建和覆盖率见
[重命名验证报告](../../reports/daemon/workspace-rename.md)。
