import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

import { AgentPermissionRequestPayloadSchema } from "./messages.js";
import { validateWSOutboundMessage } from "./validation/ws-outbound.js";
import {
  buildQuestionFormAnswers,
  parseQuestionFormQuestions,
} from "../../../apps/mobile/src/components/question-form-card-core.js";

const fixtures = JSON.parse(
  readFileSync(
    new URL("../../../crates/provider/tests/fixtures/omp_dialogs.json", import.meta.url),
    "utf8",
  ),
);

for (const fixture of fixtures) {
  test(`OMP ${fixture.name} reaches the client with usable controls and native answer keys`, () => {
    const { request, response } = fixture;
    expect(AgentPermissionRequestPayloadSchema.parse(request)).toEqual(request);
    const envelope = {
      type: "session",
      message: {
        type: "agent.stream",
        payload: {
          agentId: "agent",
          timestamp: "2026-10-28T12:00:00.000Z",
          event: { type: "permission_requested", provider: "omp", request },
        },
      },
    };
    expect(validateWSOutboundMessage(envelope)).toEqual({ success: true, data: envelope });
    if (request.kind === "tool") {
      expect(request.title).toBe(fixture.native.title);
      expect(request.actions).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ behavior: "allow" }),
          expect.objectContaining({ behavior: "deny" }),
        ]),
      );
      return;
    }
    const questions = parseQuestionFormQuestions(request.input);
    expect(questions).not.toBeNull();
    const selections: Record<number, Set<number>> = {};
    const texts: Record<number, string> = {};
    for (const [index, question] of questions!.entries()) {
      const answer = response.updatedInput.answers[question.answerKey ?? question.header];
      const values = Array.isArray(answer) ? answer : [answer];
      selections[index] = new Set<number>();
      for (const value of values) {
        const option = question.options.findIndex((option) => option.label === value);
        if (option >= 0) selections[index].add(option);
        else texts[index] = value;
      }
    }
    expect(buildQuestionFormAnswers(questions!, selections, texts)).toEqual(
      response.updatedInput.answers,
    );
  });
}
