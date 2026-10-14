import ExpoModulesCore
import UIKit

public class AitTerminalInputModule: Module {
  public func definition() -> ModuleDefinition {
    Name("AitTerminalInput")

    View(AitTerminalInputView.self) {
      Events("onInput", "onTerminalKey", "onFocus")

      AsyncFunction("showKeyboard") { (view: AitTerminalInputView, isKeyboardVisible: Bool) in
        view.input.showKeyboard(isKeyboardVisible: isKeyboardVisible)
      }

      AsyncFunction("blur") { (view: AitTerminalInputView) in
        view.input.resignFirstResponder()
        view.input.resetInput()
      }
    }
  }
}

final class AitTerminalInputView: ExpoView {
  let onInput = EventDispatcher()
  let onTerminalKey = EventDispatcher()
  let onFocus = EventDispatcher()
  let input = TerminalTextView()

  required init(appContext: AppContext? = nil) {
    super.init(appContext: appContext)
    clipsToBounds = true
    addSubview(input)
    input.onInput = { [weak self] data in self?.onInput(["data": data]) }
    input.onTerminalKey = { [weak self] key in self?.onTerminalKey(["key": key]) }
    input.onFocus = { [weak self] in self?.onFocus([:]) }
  }

  override func layoutSubviews() {
    super.layoutSubviews()
    input.frame = bounds
  }
}
