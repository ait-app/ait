import { expect, it } from "vitest";
import { commandArgumentHint } from "./command-argument-hint";

it("shows a unique visible hint only while the argument area is empty", () => {
  const entries = [
    {
      name: "run",
      aliases: ["go"],
      menuAliases: ["go"],
      description: "Run",
      argumentHint: "[path]",
    },
  ];
  expect(commandArgumentHint("/run ", entries)).toBe("[path]");
  expect(commandArgumentHint("/go \t", entries)).toBe("[path]");
  for (const input of ["/run", "/run file", "/run \n", "text /run ", "/other "]) {
    expect(commandArgumentHint(input, entries)).toBeNull();
  }
  expect(commandArgumentHint("/run ", [{ ...entries[0], menuVisible: false }])).toBeNull();
  expect(
    commandArgumentHint("/run ", [...entries, { ...entries[0], argumentHint: "[other]" }]),
  ).toBeNull();
  expect(commandArgumentHint("/run ", [{ ...entries[0], argumentHint: "x\ny" }])).toBeNull();
  expect(
    commandArgumentHint("/run ", [
      ...entries,
      { ...entries[0], source: "builtin", menuVisible: false },
    ]),
  ).toBeNull();
});
