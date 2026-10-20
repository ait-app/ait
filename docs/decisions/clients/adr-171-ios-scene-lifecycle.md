# ADR-171：iOS 窗口由 UIScene 管理

- 状态：Accepted
- 日期：2026-10-20

## 背景

Xcode 27 构建的 Ait 在 iOS 27 启动时触发
`UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`，在 JS 启动前终止。
Expo 54 的模板仍在 `AppDelegate.didFinishLaunching` 创建窗口，没有 Scene manifest。
这是独立于 xterm 初始化读取缺失 `navigator` 字段的另一处启动崩溃。

## 决策

通过 `with-ios-scene-lifecycle` 配置插件迁移生成的 AppDelegate，避免手改被忽略的
`apps/mobile/ios` 目录。Swift Scene delegate 源码保存在插件目录，prebuild 时加入已有的
AppDelegate 编译单元，不额外维护 Xcode 文件引用。

- AppDelegate 创建、绑定 React Native factory，并保留 Expo 的应用级初始化。
- Debug 的 Expo dev launcher 要求窗口和 React root 已就绪，因此仅 Debug 将 Expo
  `didFinishLaunching` 转发延后到 Scene 启动 React Native 后；Release 保持应用级初始化时机，
  包括无窗口的后台启动。
- 单个 Scene 创建 `UIWindow(windowScene:)` 后启动 React Native；粘贴模块仍在启动完成后注册。
- 冷启动 URL 和网页活动写入 React Native launch options，并在 JS 启动前经过原有
  AppDelegate 回调初始化 Expo Linking；运行期间的链接也经过相同回调。
- Scene 前后台回调转发给 Expo 54 的 AppDelegate subscribers。UIKit 自己发送的应用级通知
  不重复发送，React Native AppState 仍订阅系统通知。
- 保留 AppDelegate 的窗口引用供 Expo 54 使用；Scene 重连复用该窗口与 React Native host。
  明确关闭多窗口支持。

不升级整个 Expo SDK，也不改变账户登录或业务层边界。升级到具备官方 Scene 生命周期的
Expo SDK 时，应移除本插件并采用官方实现，避免双重转发。插件遇到不兼容的模板时直接让
prebuild 失败。

## 验证

`node --test apps/mobile/plugins/with-ios-scene-lifecycle.test.cjs` 使用当前安装的 Expo 原始
模板验证启动迁移、manifest、重复 prebuild 和粘贴插件的两种执行顺序。
原生验证需要用 Xcode 27 构建 Release，在 iOS 27 与 iOS 26 模拟器检查冷启动、
运行中打开 `ait://settings/general` 和退到后台后返回。模拟器验证不替代真机的浏览器登录
与推送通知验收。

## 参考

- [Apple：迁移到 Scene 生命周期](https://developer.apple.com/documentation/uikit/transitioning-to-the-uikit-scene-based-life-cycle)
- [Expo：iOS Scene 生命周期迁移](https://github.com/expo/fyi/blob/main/ios-scene-lifecycle.md)
