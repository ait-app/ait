import { expect, it } from "vitest";
import { insertDirectoryPaths } from "./directory-drop";

it("inserts multiple literal directory paths at the selection while preserving surrounding text", () => {
  const result = insertDirectoryPaths(
    { text: "Before replace after", selection: { start: 7, end: 14 } },
    ["/home/me/课程 作业", "C:\\Work Files\\folder"],
  );
  expect(result.text).toBe("Before /home/me/课程 作业\nC:\\Work Files\\folder\n after");
  expect(result.selection.start).toBe(result.text.indexOf(" after"));
  expect(result.selection.end).toBe(result.selection.start);
});

it("separates a dropped path from existing draft text and leaves an empty drop alone", () => {
  const input = { text: "Inspect", selection: { start: 7, end: 7 } };
  expect(insertDirectoryPaths(input, ["/tmp/a"]).text).toBe("Inspect\n/tmp/a\n");
  expect(insertDirectoryPaths(input, [])).toEqual(input);
});
