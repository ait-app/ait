# ADR-150：Android 边缘手势进入移动面板导航

- 状态：已实现，待真机验收
- 日期：2026-10-15

## 背景

移动端内部横滑通过 Gesture Handler 打开左右面板。Android 将左右边缘向内横滑识别为系统返回，取消应用的触摸序列；这条路径没有进入面板逻辑，根页面会退到桌面。原先只有 Workspace 的文件面板单独监听返回，左侧列表没有对应处理。

## 决策

- Android 原生适配归本地 Expo 模块 `ait-panel-gestures`。模块只报告完成的系统返回来自哪一侧，并转交 React Native 现有的 `hardwareBackPress`；面板状态、打开权限和工作区文件面板回调仍由 `mobile-panels` 拥有。
- 紧凑布局且应用导航可用时安装监听；退出该布局、切后台或销毁模块时移除原生回调。iOS、Web 和宽布局不安装面板返回监听。
- Android 14+ 使用 `OnBackAnimationCallback` 的 `swipeEdge` 区分左右，取消手势清除方向。启用 Expo 的 `predictiveBackGestureEnabled`；原生回调保持默认优先级，让输入法先处理返回。边缘触摸开始时重新注册回调，避免导航后 Fragment 新增的同优先级回调抢先丢失方向。
- Android 10–13 没有预测返回的方向回调，使用系统取消的边缘触摸作为兼容来源，按系统手势 Insets 判断边缘，消费一次且最多保留 2.5 秒。正常结束、内部触摸、多指、取消预测返回及生命周期退出均清除记录。Android 13+ 的按键事件额外清除方向，避免继承取消的手势。
- 左边缘右滑在中心页面打开 Agent 列表，右边缘左滑调用当前已启用的文件面板打开命令；反向滑动关闭对应面板。无法打开的方向以及被当前内容禁止的横滑仍消费系统返回，避免退到桌面。
- 统一在移动导航手势入口处理两个面板，删除 Workspace 单独的返回监听。返回按钮先关闭当前移动面板，中心页面继续交给导航。新打开的弹层仍通过现有 BackHandler 链先处理返回。

## 后果与限制

- 需要重新 prebuild 并构建 Android 安装包；只更新 JavaScript 无法加入原生模块。
- 面板最终目标与内部横滑一致；系统边缘手势仍使用系统识别及提交时机，完成后启动现有面板动画，不提供内部横滑的逐帧跟手动画。
- Android 10–13 的兼容方向依赖系统向应用派发触摸取消，不能替代 Android 14+ 的明确方向 API；系统取消手势后立刻按返回键，在 Android 10–12 上可能仍被归为刚才的边缘方向。需在支持的旧设备上验收。
- 不采用全屏手势排除矩形：Android 对每侧可排除高度设有 200dp 限制，不能据此承诺整条边缘都进入应用触摸手势。

## 验证

70 项相关 TypeScript 回归通过真实移动手势 Hook、面板 Store 及相关状态模型检查左右打开、关闭、横滑阻止、返回按钮、弹层优先处理及监听释放。最终 Kotlin 源码已针对 Android 36、React Native 与 Expo Core 实际依赖编译通过，4 项 Kotlin 回归检查旧系统的方向记录、一次消费、过期与清理。

移动端类型检查、变更文件的格式和 UI lint、文档链接检查通过。Android lint 在依赖分析时受本机磁盘空间及 Gradle 缓存锁问题阻塞，未完成；不将其计为通过。真机 USB 调试暂未授权，本次不声称完成真机验收。未改动 Rust，按仓库规则未运行 Rust 测试。

## 参考

- [Android 手势导航与冲突处理](https://developer.android.com/develop/ui/views/touch-and-input/gestures/gesturenav)
- [Android 预测返回回调](https://developer.android.com/reference/android/window/OnBackAnimationCallback)
- [Android 手势排除高度限制](https://codelabs.developers.google.com/codelabs/gesture-navigation)
