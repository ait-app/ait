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

  test("secret and whitespace-preserving inputs keep answers verbatim", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "API token",
          header: "API token",
          options: [],
          allowOther: true,
          allowEmpty: true,
          secret: true,
          preserveWhitespace: true,
          dismissBehavior: "deny",
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(questions[0].secret).toBe(true);
    expect(questions[0].dismissBehavior).toBe("deny");
    expect(buildQuestionFormAnswers(questions, {}, { 0: "  hunter2  " })).toEqual({
      "API token": "  hunter2  ",
    });
    expect(areQuestionsAnswered(questions, {}, {})).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, {})).toEqual({ "API token": "" });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(false);
  });

  test("editor prefill seeds the answer and keeps newlines", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Edit the plan",
          header: "Edit the plan",
          options: [],
          allowOther: true,
          allowEmpty: true,
          multiline: true,
          initialValue: "Initial plan",
          dismissBehavior: "deny",
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(areQuestionsAnswered(questions, {}, {})).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, {})).toEqual({
      "Edit the plan": "Initial plan",
    });
    expect(buildQuestionFormAnswers(questions, {}, { 0: "Updated\nplan" })).toEqual({
      "Edit the plan": "Updated\nplan",
    });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(false);
  });

  test("keeps multiline text verbatim, including a trailing newline", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Edit the plan",
          header: "Edit the plan",
          options: [],
          allowOther: true,
          allowEmpty: false,
          multiline: true,
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(areQuestionsAnswered(questions, {}, {})).toBe(false);
    expect(areQuestionsAnswered(questions, {}, { 0: "  line one\nline two\n" })).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, { 0: "  line one\nline two\n" })).toEqual({
      "Edit the plan": "  line one\nline two\n",
    });
  });

  test("rejects an empty secret submit but accepts a whitespace value verbatim", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "API token",
          header: "API token",
          options: [],
          allowOther: true,
          allowEmpty: false,
          secret: true,
          preserveWhitespace: true,
          dismissBehavior: "deny",
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(areQuestionsAnswered(questions, {}, {})).toBe(false);
    expect(areQuestionsAnswered(questions, {}, { 0: "" })).toBe(false);
    expect(buildQuestionFormAnswers(questions, {}, {})).toEqual({});
    expect(areQuestionsAnswered(questions, {}, { 0: " " })).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, { 0: " " })).toEqual({ "API token": " " });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(false);
  });

  test("rejects whitespace-only single-line answers without allowEmpty", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Branch name",
          header: "Branch name",
          options: [],
          allowOther: true,
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(areQuestionsAnswered(questions, {}, { 0: "   " })).toBe(false);
    expect(buildQuestionFormAnswers(questions, {}, { 0: "   " })).toEqual({});
    expect(areQuestionsAnswered(questions, {}, { 0: " main " })).toBe(true);
    expect(buildQuestionFormAnswers(questions, {}, { 0: " main " })).toEqual({
      "Branch name": "main",
    });
  });

  test("plain inputs still trim and may submit empty on dismiss", () => {
    const questions = parseQuestionFormQuestions({
      questions: [
        {
          question: "Optional note",
          header: "Optional note",
          options: [],
          allowOther: true,
          allowEmpty: true,
        },
      ],
    });
    if (!questions) throw new Error("questions did not parse");
    expect(buildQuestionFormAnswers(questions, {}, { 0: "  note  " })).toEqual({
      "Optional note": "note",
    });
    expect(shouldSubmitEmptyOnDismiss(questions)).toBe(true);
  });
});
