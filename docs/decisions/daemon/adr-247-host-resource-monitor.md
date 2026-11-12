---
id: ADR-247
status: accepted
date: 2026-11-11
paths:
  - crates/metadata/src/daemon
  - bins/daemon/src/host.rs
  - packages/client
  - packages/protocol/src/host-monitor.ts
  - apps/mobile/src/host-monitor
---

# ADR-247：按需采样主机资源并在侧栏展示

## 背景

用户需要从当前工作区确认 daemon 所在主机的资源状态。Ait 已移除旧插件运行时，
需要沿用原生服务与共享客户端协议提供有界的监控能力。

## 决策

主机监控属于 `metadata::daemon`。`ports::host_monitor::HostSampler` 声明同步采样端口及
类型，`local::LocalHostSampler` 使用 sysinfo 读取系统计数；daemon 组装时注入适配器。
应用服务拥有缓存与历史，沿用既有 blocking worker 执行，不在 Tokio reactor 上读取系统。
不恢复 Paseo 插件运行环境，不启动 Node 子进程，不读取进程列表或持久化指标。

新增鉴权 RPC `daemon.host_stats.request`，参数 `includeHistory` 默认为 false。
所有连接共享最短两秒的采样间隔，磁盘每三十秒更新；只有收到读取请求才采样。
历史同时限制为最近三分钟、最多九十点。CPU 和网络速率的首次观测、间隔超过十秒的
首次恢复均为 null，下一次采样才给出速率。缺失指标与真实零值分别表达。

共享协议、SDK 和 Rust transport 一起声明方法。客户端根据协商结果生成 `hostStats`
能力标志；旧主机不发送新请求，显示升级提示。缓存按 serverId 与是否包含历史分开，
详情关闭后只读取摘要，页面不可见、侧栏关闭或主机离线时暂停刷新。

Desktop 左侧栏在用量摘要与底部工具栏之间显示主机名、CPU、内存。点击后使用现有
浮层引擎向右展开，窄屏使用底部抽屉。默认跟随当前／最近工作区主机，也可手动固定
主机；手动选择保留到该侧栏组件卸载。固定主机离线时不切换到另一台机器。
刷新失败和离线时摘要显示破折号，详情保留最后采样和明确的时间。历史图按真实时间
绘制，有较长采样间隔时断开；样本不足时显示积累中。

## 后果及限制

指标来自 daemon 所在操作系统，容器环境不保证等同于容器配额。内存沿用 sysinfo
平台口径，不宣称与 macOS Activity Monitor 完全一致；本版不采集 memory pressure，
内存百分比保持中性色。网速排除 loopback 与常见重复计数的虚拟网卡。
磁盘展示普通用户的可用容量及总容量，避免把保留块当作实际文件占用；Linux 默认
排除 NFS/CIFS 等网络文件系统并按设备去重，macOS 只显示根 APFS 容器。
历史仅覆盖用户查看期间，不补造停采期间的数据。macOS 的实际采样需在对应机器验证。

## 界面示例

共享界面的桌面布局：[浅色主题](../../assets/host-monitor/desktop-light.png)、
[深色主题](../../assets/host-monitor/desktop-dark.png)。截图使用隔离测试主机的真实数据。
