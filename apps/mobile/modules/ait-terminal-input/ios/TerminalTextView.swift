import UIKit

// This view owns its buffer. In particular, React renders never replace text
// or typing attributes while UIKit is converting a marked (unconfirmed) word.
final class TerminalTextView: UITextView, UITextViewDelegate {
  var onInput: ((String) -> Void)?
  var onTerminalKey: ((String) -> Void)?
  var onFocus: (() -> Void)?

  private var committedText = ""
  private var mutationDepth = 0
  private var isResetting = false

  init() {
    super.init(frame: .zero, textContainer: nil)
    delegate = self
    backgroundColor = .clear
    textColor = .clear
    tintColor = .clear
    font = .monospacedSystemFont(ofSize: 16, weight: .regular)
    textContainerInset = .zero
    textContainer.lineFragmentPadding = 0
    isScrollEnabled = false
    autocapitalizationType = .none
    autocorrectionType = .no
    spellCheckingType = .no
    smartQuotesType = .no
    smartDashesType = .no
    smartInsertDeleteType = .no
    keyboardType = .default
    textContentType = nil
    accessibilityLabel = "Terminal input"
    accessibilityIdentifier = "terminal-native-input"
  }

  required init?(coder: NSCoder) {
    fatalError("init(coder:) has not been implemented")
  }

  func showKeyboard(isKeyboardVisible: Bool) {
    // A repeated terminal tap must preserve an active candidate, while a
    // dismissed keyboard needs a new first-responder transition to reopen.
    if isFirstResponder && isKeyboardVisible { return }
    if isFirstResponder { resignFirstResponder() }
    becomeFirstResponder()
  }

  func resetInput() {
    isResetting = true
    super.unmarkText()
    text = ""
    committedText = ""
    isResetting = false
  }

  override func setMarkedText(_ markedText: String?, selectedRange: NSRange) {
    mutateInput { super.setMarkedText(markedText, selectedRange: selectedRange) }
  }

  override func unmarkText() {
    mutateInput { super.unmarkText() }
  }

  override func insertText(_ text: String) {
    if text == "\n" || text == "\r" {
      if markedTextRange != nil {
        // Return confirms the candidate. A subsequent Return submits the line.
        unmarkText()
      } else {
        onInput?("\r")
        resetInput()
      }
      return
    }
    // Multiline clipboard input goes through the terminal's explicit paste
    // action, which applies bracketed-paste handling before reaching the PTY.
    guard !text.contains("\n") && !text.contains("\r") else { return }
    mutateInput { super.insertText(text) }
  }

  override func replace(_ range: UITextRange, withText text: String) {
    guard !text.contains("\n") && !text.contains("\r") else { return }
    mutateInput { super.replace(range, withText: text) }
  }

  override func deleteBackward() {
    if markedTextRange == nil && text.isEmpty {
      onInput?("\u{7f}")
      return
    }
    // Deleting a phonetic reading edits only the IME buffer, never the PTY.
    mutateInput { super.deleteBackward() }
  }

  private func mutateInput(_ mutation: () -> Void) {
    mutationDepth += 1
    mutation()
    mutationDepth -= 1
    flushCommittedText()
  }

  private func flushCommittedText() {
    guard mutationDepth == 0 && !isResetting else { return }
    let buffer = text ?? ""
    if buffer.contains("\n") || buffer.contains("\r") {
      if markedTextRange == nil { resetInput() }
      return
    }
    let confirmed: String
    if let markedRange = markedTextRange {
      let offset = self.offset(from: beginningOfDocument, to: markedRange.start)
      confirmed = (buffer as NSString).substring(to: offset)
    } else {
      confirmed = buffer
    }

    // Never translate a native correction into deletion of already-sent PTY
    // content. Only confirmed appends are terminal input.
    if confirmed.hasPrefix(committedText) {
      let appended = String(confirmed.dropFirst(committedText.count))
      if !appended.isEmpty { onInput?(appended) }
    }
    committedText = confirmed
    if markedTextRange == nil { resetInput() }
  }

  func textViewDidChange(_ textView: UITextView) {
    flushCommittedText()
  }

  func textViewDidChangeSelection(_ textView: UITextView) {
    flushCommittedText()
  }

  func textViewDidBeginEditing(_ textView: UITextView) {
    onFocus?()
  }

  func textViewDidEndEditing(_ textView: UITextView) {
    resetInput()
  }

  override var keyCommands: [UIKeyCommand]? {
    // Let the IME use arrows to navigate candidates during conversion.
    guard markedTextRange == nil else { return super.keyCommands }
    let arrows = [
      UIKeyCommand.inputUpArrow, UIKeyCommand.inputDownArrow,
      UIKeyCommand.inputLeftArrow, UIKeyCommand.inputRightArrow,
    ]
    return (super.keyCommands ?? [])
      + arrows.map { arrow in
        let command = UIKeyCommand(
          input: arrow, modifierFlags: [], action: #selector(handleArrow(_:)))
        command.wantsPriorityOverSystemBehavior = true
        return command
      }
  }

  @objc private func handleArrow(_ command: UIKeyCommand) {
    guard markedTextRange == nil else { return }
    switch command.input {
    case UIKeyCommand.inputUpArrow: onTerminalKey?("ArrowUp")
    case UIKeyCommand.inputDownArrow: onTerminalKey?("ArrowDown")
    case UIKeyCommand.inputLeftArrow: onTerminalKey?("ArrowLeft")
    case UIKeyCommand.inputRightArrow: onTerminalKey?("ArrowRight")
    default: break
    }
  }

  override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
    false
  }
}
