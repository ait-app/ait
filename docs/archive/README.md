# 历史归档

这里保存已完成或被取代的方案及早期验证证据。**正文保留当时的命令、路径、平台和结论，不是当前操作指南。**
测试数量、覆盖率和部署结论仅对应原记录的提交与环境；需要复现旧源码时使用 Git 历史。
归档整理日期：2026-11-03。

## 方案与替代入口

| 历史资料                                                         | 归档原因                                      | 当前入口 / 未完成工作                                                                                            |
| ---------------------------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| [Linux/macOS 设备授权](plans/linux-headless-device-auth.md)      | 旧双模式与 `--headless` 已被统一发布取代      | [操作手册](../operations/headless-daemon.md)、[ADR-201](../decisions/daemon/adr-201-daemon-owned-publication.md) |
| [Paseo 并发改造](plans/paseo-agent-concurrency.md)               | 实现已完成；采样与平台验收已合并              | [性能待办](../plans/remote-workspace-open-performance.md)                                                        |
| [Provider parity](plans/provider-parity.md)                      | 固定上游版本的交付清单已完成                  | [Provider 操作入口](../README.md#agent-与工作区)、[Provider ADR](../decisions/README.md#provideragent-与会话)    |
| [Bot 原设计与验证](plans/bot-backend.md)                         | 混合了已实现接口、历史测试与 Email 设想       | [Bot 手册](../operations/bots.md)、[后续工作](../plans/README.md)                                                |
| [远程打开原分析](plans/remote-workspace-open-performance.md)     | 改造前调用链、旧 Relay 与估算不再代表当前实现 | [性能待办](../plans/remote-workspace-open-performance.md)                                                        |
| [2026-10-10 登录保持](plans/relay-client-session-persistence.md) | 期限调整和旧客户端租约流程属于历史            | [当前登录](../operations/authing-client-login.md)、[续期待办](../plans/relay-client-session-persistence.md)      |
| [统一登录旧部署](operations/authing-client-login.md)             | 历史数据库迁移、固定期限和旧租约语义          | [当前登录](../operations/authing-client-login.md)                                                                |
| [旧 Relay 诊断](operations/relay-diagnostics.md)                 | 业务桥接与 Yamux 已退役                       | [Hub 与下载 Relay 诊断](../operations/relay-diagnostics.md)                                                      |

## 早期验证与发布

- [Paseo WebSocket 移植阶段 1–12](reports/daemon/README.md)：保留阶段报告及原始 JSON 覆盖率附件。
- [0.0.x 发布记录](reports/releases/README.md)：保留发布准备、失败、重建及验证附件；当前发布方法见[发布指南](../operations/releasing.md)。
- 后续版本和仍用于定位行为的专题证据继续按能力保存在[报告目录](../reports/README.md)。

ADR 保留在原分类下，通过状态和修订关系说明演进；不因编号较早而归档仍有效的边界。
来源声明、许可证和仍被当前品牌决策引用的资产继续保留。
