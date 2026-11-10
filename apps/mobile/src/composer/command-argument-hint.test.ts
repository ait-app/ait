import { expect, it } from "vitest";
import { commandArgumentHint } from "./command-argument-hint";

const commands = [
  {
    name: "guided-goal",
    description: "Interview",
    argumentHint: "[rough objective]",
    source: "ait",
  },
  {
    name: "track",
    aliases: ["work"],
    description: "Plugin",
    argumentHint: "[duration] [work]",
    source: "extension",
  },
];
it("uses actual command metadata and aliases without changing the input", () => {
  const draft = "/guided-goal ";
  expect(commandArgumentHint(draft, commands)).toEqual({
    prefix: draft,
    hint: "[rough objective]",
  });
  expect(commandArgumentHint("/work ", commands)?.hint).toBe("[duration] [work]");
  expect(draft).toBe("/guided-goal ");
});
it("does not invent hints or cover typed arguments, multiline content or paths", () => {
  for (const text of [
    "/unknown ",
    "/guided-goal x",
    "/guided-goal\n",
    "/etc/hosts ",
    "/guided-goal",
  ])
    expect(commandArgumentHint(text, commands)).toBeNull();
  expect(commandArgumentHint("/track ", [{ ...commands[1]!, argumentHint: "" }])).toBeNull();
});
