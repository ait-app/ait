# ADR-114：iOS 终端组合输入归 UIKit

- 状态：已实现
- 日期：2026-10-09

## 背景

iOS 原生终端用 React Native 隐藏输入框同时处理 `onKeyPress` 和 `onChangeText`。
输入法尚未确认的拼音先进入 PTY，选字后再用退格替换。中间的拼音分词、分隔符改写
可能被当成普通自动纠错，从而禁用后续候选替换；全屏终端程序也无法可靠撤销已处理的按键。
只有字符串差异，没有组合范围，无法区分未确认的输入与已经提交的文字。

Apple 的 [UITextInput](https://developer.apple.com/documentation/uikit/uitextinput)
用 `markedTextRange` 表示尚未确认的组合文字。React Native Fabric 的组合状态还存在
[上游问题报告](https://github.com/react/react-native/issues/56463)。本修复不依赖上游补丁。

## 决策

- 新增本地 Expo 模块 `ait-terminal-input`，仅用于 iOS 原生终端。UIKit `UITextView`
  持有输入缓冲区和组合范围，React 的重新渲染不写回文字或输入属性。
- 组合文字只留在本地；确认的文字通过 `onInput` 一次发送到既有终端输入链路。
  原生变更期间合并嵌套的文字、选区回调，避免候选转换中临时状态提前提交。
- 组合期间的退格、方向键用于编辑和选词，回车确认候选。组合结束后的退格、回车和
  方向键继续发送终端控制输入。重复聚焦保留正在编辑的候选；失焦取消未确认输入。
- 多行粘贴沿用显式终端粘贴入口，不从隐藏输入框发送原始多行文字。
- 用平台入口选择 iOS 组件；Android 保留现有实现，Web 和旧 WebView 渲染器继续使用
  xterm 输入链路。共享终端、协议和 Rust daemon 不变。

## 后果

iOS 不再通过回删拼音来提交中文，拼音分隔符和候选改写不会影响 PTY 已收到的内容。
新增原生模块需要重新构建 iOS 安装包，仅更新 JavaScript 无法给旧二进制补入原生组件。
Android 的字符串推测仍有同类风险，需要另行验证实际输入法事件。

## 验证

UIKit 回归位于 `apps/mobile/modules/ait-terminal-input/tests`，直接在模拟器中执行
组合、选字、取消、退格、回车、焦点和方向键事件，不需要后端或完整 Expo App：

```sh
ruby apps/mobile/modules/ait-terminal-input/tests/run-ios-tests.rb <simulator-udid>
```

需要 Xcode、可用 iOS 模拟器以及 CocoaPods 提供的 Ruby `xcodeproj`。工程和构建产物
放在临时目录，测试结束后清理。JavaScript 桥接测试位于
`apps/mobile/src/terminal/native-renderer/terminal-input.ios.test.tsx`。
