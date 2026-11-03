import { describe, expect, test } from "vitest";
import {
  areQuestionsAnswered,
  buildQuestionFormAnswers,
  parseQuestionFormQuestions,
  questionShowsTextInput,
  resolveDismissLabel,
  shouldSubmitEmptyOnDismiss,
} from "./question-form-card-core";

describe("question form card core", () => {
  test("native editor prefill can be submitted unchanged, edited verbatim, or cleared", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          header: "Editor",
          question: "Edit config",
          options: [],
          allowOther: true,
          multiline: true,
          initialValue: "  initial\n",
          allowEmpty: true,
          dismissBehavior: "deny",
        },
      ],
    })!;
    expect(areQuestionsAnswered(questions, {}, {})).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, {})).toEqual({ Editor: "  initial\n" });
    expect(buildQuestionFormAnswers(questions, {}, { 0: "  revised\n\n" })).toEqual({
      Editor: "  revised\n\n",
    });
    expect(buildQuestionFormAnswers(questions, {}, { 0: "" })).toEqual({ Editor: "" });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(false);
  });

  test("secret and verbatim inputs preserve spaces while ordinary answers keep their trimming", () => {
    for (const metadata of [{ secret: true }, { preserveWhitespace: true }, {}]) {
      const questions = parseQuestionFormQuestions({
        questions: [
          {
            header: "Value",
            question: "Enter a value",
            options: [],
            allowOther: true,
            ...metadata,
          },
        ],
      })!;
      expect(buildQuestionFormAnswers(questions, {}, { 0: "  value  " })).toEqual({
        Value: Object.keys(metadata).length ? "  value  " : "value",
      });
    }
  });
  test("keeps explicit multiline editor text when generating native answers", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          header: "Plan",
          question: "Edit the plan",
          options: [],
          allowOther: true,
          multiline: true,
        },
      ],
    });
    expect(questions?.[0].multiline).toBe(true);
    expect(buildQuestionFormAnswers(questions!, {}, { 0: "First line\nSecond line" })).toEqual({
      Plan: "First line\nSecond line",
    });
  });
  test("treats optional input prompts as skippable empty answers", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Optional comment?",
          header: "Response",
          options: [],
          multiSelect: false,
          placeholder: "Optional comment (press Enter to skip)...",
          allowEmpty: true,
          dismissLabel: "Skip",
        },
      ],
    });

    if (!questions) throw new Error("questions did not parse");
    expect(areQuestionsAnswered(questions, {}, {})).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, {})).toEqual({ Response: "" });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(true);
    expect(resolveDismissLabel(questions)).toBe("Skip");
  });

  test("requires a selection for option-only questions", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Pick one",
          header: "Response",
          options: [{ label: "A" }, { label: "B" }],
          multiSelect: false,
        },
      ],
    });

    if (!questions) throw new Error("questions did not parse");
    const [question] = questions;
    if (!question) throw new Error("question missing");
    expect(questionShowsTextInput(question)).toBe(false);
    expect(areQuestionsAnswered(questions, {}, { 0: "freeform" })).toBe(false);
    expect(areQuestionsAnswered(questions, { 0: new Set([1]) }, {})).toBe(true);
    expect(buildQuestionFormAnswers(questions, { 0: new Set([1]) }, {})).toEqual({
      Response: "B",
    });
  });

  test("shows text input for explicit other questions", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Pick or type",
          header: "Response",
          options: [{ label: "A" }],
          isOther: true,
          multiSelect: false,
        },
      ],
    });

    if (!questions) throw new Error("questions did not parse");
    const [question] = questions;
    if (!question) throw new Error("question missing");
    expect(questionShowsTextInput(question)).toBe(true);
    expect(areQuestionsAnswered(questions, {}, { 0: "custom" })).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, { 0: "custom" })).toEqual({
      Response: "custom",
    });
  });

  test("shows text input for questions that allow other answers", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Pick or type",
          header: "Response",
          options: [{ label: "A" }],
          allowOther: true,
          multiSelect: false,
        },
      ],
    });

    if (!questions) throw new Error("questions did not parse");
    const [question] = questions;
    if (!question) throw new Error("question missing");
    expect(questionShowsTextInput(question)).toBe(true);
    expect(areQuestionsAnswered(questions, {}, { 0: "custom" })).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, { 0: "custom" })).toEqual({
      Response: "custom",
    });
  });
});

describe("structured native question answers", () => {
  test("submits a selected label only once when custom text matches it", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Choose a test answer",
          header: "Answer",
          answerKey: "answer",
          options: [{ label: "Alpha, beta" }, { label: "Gamma" }],
          multiSelect: true,
          allowOther: true,
          answerFormat: "array",
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(
      buildQuestionFormAnswers(questions, { 0: new Set([0, 1]) }, { 0: " Alpha, beta " }),
    ).toEqual({ answer: ["Alpha, beta", "Gamma"] });
    expect(buildQuestionFormAnswers(questions, { 0: new Set([0]) }, { 0: "alpha, beta" })).toEqual({
      answer: ["Alpha, beta", "alpha, beta"],
    });
  });

  test("preserves commas inside labels and selected choices alongside custom text", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Choose targets",
          header: "Targets",
          answerKey: "targets",
          options: [{ label: "Linux, amd64" }, { label: "macOS" }],
          multiSelect: true,
          allowOther: true,
          answerFormat: "array",
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(buildQuestionFormAnswers(questions, { 0: new Set([0, 1]) }, { 0: "Windows" })).toEqual({
      targets: ["Linux, amd64", "macOS", "Windows"],
    });
  });
});
