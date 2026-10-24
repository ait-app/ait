# ADR-189：Pi 不暴露 Host 模式

- 状态：Accepted
- 日期：2026-10-24
- 修订：[ADR-156](adr-156-pi-native-rpc-provider.md) 的合成 Pi 模式。

## 背景

Pi 的工具、扩展和设置始终由 Pi 自己管理。Ait 曾经为 Pi 合成过唯一的 `default` 模式，界面上
显示为 `Native Tools`，但选中它并不会改变 Pi 的权限、工具或沙箱行为。这说明它并不是用户可以
切换的运行策略；继续显示模式选择器，只会让人误以为 Ait 能管控 Pi 的权限边界。

## 决策

1. Pi 的静态清单、运行时设置和动态目录都返回空的模式列表，静态 `defaultModeId` 为 `null`。
2. 新建会话、运行时信息和原生历史导入都不再写入 `modeId`。Pi 继续按原样使用自己的工具、扩展
   和设置；Ait 不叠加自己的权限提示，也不引入沙箱。
3. 为了让已持久化的 Agent 仍能恢复，Pi 的校验继续接受旧的 `default` 值，但只把它当作无副作用的
   输入：不再对外声明，不再提供给用户选择，也不再输出它。其他模式值仍然被拒绝。

## 后果

客户端不再为 Pi 显示模式选择器，也不会把 Pi 归类为支持 unattended 模式的 Provider。已有的 Pi
Agent 可以照常恢复，并在会话运行时转为无模式状态；原生工具的行为保持不变。

## Test coverage

未测量：用户未请求覆盖率。如需测量，把范围限定在 Pi adapter 与相关服务测试，使用
`cargo llvm-cov`，并记录修订版本、执行命令和共享报告。

定向回归结果：Pi adapter 定向测试 125 个全部通过；默认被跳过的 installed Pi 测试以 Pi 1.2.0
单独执行，同样通过；Agent manager 102 个通过，Agent execution 99 个通过，共享 Provider
manifest 5 个通过。相关客户端测试 153 个通过、0 失败。workspace build、Clippy、protocol 与
mobile typecheck、格式检查与文档链接检查全部通过。

完整环境、执行命令和已知限制见[验证报告](../../reports/providers/pi-mode-removal-2026-10-24.md)。
