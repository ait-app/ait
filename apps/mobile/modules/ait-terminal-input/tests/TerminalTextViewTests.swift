import UIKit
import XCTest

@MainActor
final class TerminalTextViewTests: XCTestCase {
  private var window: UIWindow!
  private var input: TerminalTextView!
  private var inputs: [String] = []
  private var keys: [String] = []

  override func setUp() {
    super.setUp()
    inputs = []
    keys = []
    window = UIWindow(frame: UIScreen.main.bounds)
    window.rootViewController = UIViewController()
    input = TerminalTextView()
    input.frame = CGRect(x: 0, y: 0, width: 320, height: 48)
    input.onInput = { [weak self] in self?.inputs.append($0) }
    input.onTerminalKey = { [weak self] in self?.keys.append($0) }
    window.rootViewController!.view.addSubview(input)
    window.makeKeyAndVisible()
    input.becomeFirstResponder()
  }

  override func tearDown() {
    input.resignFirstResponder()
    window.isHidden = true
    input = nil
    window = nil
    super.tearDown()
  }

  private func compose(_ text: String) {
    input.setMarkedText(text, selectedRange: NSRange(location: text.utf16.count, length: 0))
  }

  func testPinyinRewritesStayLocalUntilCandidateCommit() {
    for reading in ["n", "ni", "ni h", "ni hao", "ni'hao", "nihao"] {
      compose(reading)
      XCTAssertNotNil(input.markedTextRange)
      XCTAssertEqual(inputs, [])
    }
    input.insertText("你好")
    XCTAssertEqual(inputs, ["你好"])
    XCTAssertNil(input.markedTextRange)
    XCTAssertEqual(input.text, "")
  }

  func testChangingCandidatesDoesNotSendOrDeleteTerminalText() {
    compose("nihao")
    compose("你好")
    compose("拟好")
    XCTAssertEqual(inputs, [])
    input.unmarkText()
    XCTAssertEqual(inputs, ["拟好"])
    input.unmarkText()
    XCTAssertEqual(inputs, ["拟好"])
  }

  func testReturnConfirmsCompositionBeforeSubmittingTerminalLine() {
    compose("你好")
    input.insertText("\n")
    XCTAssertEqual(inputs, ["你好"])
    input.insertText("\n")
    XCTAssertEqual(inputs, ["你好", "\r"])
    XCTAssertTrue(input.isFirstResponder)
  }

  func testBackspaceEditsReadingWithoutDeletingCommittedTerminalText() {
    input.insertText("echo ")
    compose("ni")
    // IMEs shorten the marked reading instead of deleting PTY content.
    compose("n")
    XCTAssertEqual(inputs, ["echo "])
    compose("")
    XCTAssertEqual(inputs, ["echo "])
    input.unmarkText()
    input.deleteBackward()
    XCTAssertEqual(inputs, ["echo ", "\u{7f}"])
  }

  func testNativeDeleteBackwardCanCancelTheWholeMarkedReading() {
    input.insertText("echo ")
    compose("ni")
    // UIKit's direct deleteBackward removes the whole marked selection.
    input.deleteBackward()
    XCTAssertEqual(inputs, ["echo "])
    input.unmarkText()
    XCTAssertEqual(inputs, ["echo "])
    input.deleteBackward()
    XCTAssertEqual(inputs, ["echo ", "\u{7f}"])
  }

  func testCommittedChineseCanBeDeletedWithOneTerminalBackspace() {
    compose("ni")
    input.insertText("你")
    input.deleteBackward()
    XCTAssertEqual(inputs, ["你", "\u{7f}"])
  }

  func testMixedEnglishChineseAndEmojiAreSentOnce() {
    input.insertText("echo ")
    compose("ni")
    input.insertText("你")
    compose("hao")
    input.insertText("好")
    input.insertText("🙂")
    XCTAssertEqual(inputs, ["echo ", "你", "好", "🙂"])
  }

  func testJapaneseAndKoreanConversionIsNotEmittedProvisionally() {
    for reading in ["に", "にほんご", "日本語"] { compose(reading) }
    XCTAssertEqual(inputs, [])
    input.unmarkText()
    for syllable in ["ㅎ", "하", "한"] { compose(syllable) }
    XCTAssertEqual(inputs, ["日本語"])
    input.unmarkText()
    XCTAssertEqual(inputs, ["日本語", "한"])
  }

  func testRepeatedFocusDoesNotClearMarkedText() {
    compose("nihao")
    input.showKeyboard(isKeyboardVisible: true)
    XCTAssertNotNil(input.markedTextRange)
    XCTAssertEqual(input.text, "nihao")
    XCTAssertEqual(inputs, [])
    input.insertText("你好")
    XCTAssertEqual(inputs, ["你好"])
  }

  func testOnePixelHiddenInputCommitsChinese() {
    input.frame = CGRect(x: 0, y: 0, width: 1, height: 1)
    compose("ni'hao")
    input.layoutIfNeeded()
    XCTAssertEqual(inputs, [])
    input.insertText("你好")
    XCTAssertEqual(inputs, ["你好"])
    XCTAssertNil(input.markedTextRange)
  }

  func testCancelingCompositionDoesNotReplayItAfterRefocus() {
    compose("nihao")
    input.resignFirstResponder()
    input.showKeyboard(isKeyboardVisible: false)
    input.insertText("x")
    XCTAssertEqual(inputs, ["x"])
    XCTAssertNil(input.markedTextRange)
  }

  func testPlainTypingAndReturnKeepKeyboardFocused() {
    input.insertText("p")
    input.insertText("w")
    input.insertText("d")
    input.insertText("\n")
    XCTAssertEqual(inputs, ["p", "w", "d", "\r"])
    XCTAssertTrue(input.isFirstResponder)
  }

  func testMultilineHiddenInputPasteIsNotSentRaw() {
    input.insertText("one\ntwo")
    input.insertText("one\rtwo")
    let range = input.textRange(from: input.beginningOfDocument, to: input.endOfDocument)!
    input.replace(range, withText: "one\ntwo")
    XCTAssertEqual(inputs, [])
    input.insertText("safe")
    XCTAssertEqual(inputs, ["safe"])
  }

  func testArrowKeysBelongToTerminalOnlyOutsideComposition() {
    let commands = input.keyCommands!.filter { $0.wantsPriorityOverSystemBehavior }
    for command in commands { input.perform(command.action, with: command) }
    XCTAssertEqual(keys, ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"])
    compose("ni")
    for command in commands { input.perform(command.action, with: command) }
    XCTAssertEqual(keys, ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"])
    XCTAssertEqual(inputs, [])
  }
}
