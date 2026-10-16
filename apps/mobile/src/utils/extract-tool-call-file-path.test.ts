import { describe, expect, it } from "vitest";
import { extractToolCallFilePath } from "./extract-tool-call-file-path";

describe("extractToolCallFilePath", () => {
  it("returns filePath for read/edit/write", () => {
    expect(extractToolCallFilePath({ type: "read", filePath: "/a.ts" })).toBe("/a.ts");
    expect(extractToolCallFilePath({ type: "edit", filePath: "/b.ts" })).toBe("/b.ts");
    expect(extractToolCallFilePath({ type: "write", filePath: "/c.ts" })).toBe("/c.ts");
  });

  it("returns null for empty filePath", () => {
    expect(extractToolCallFilePath({ type: "read", filePath: "" })).toBeNull();
  });

  it("does not open the synthetic path in legacy multi-file edit cards", () => {
    expect(
      extractToolCallFilePath({
        type: "edit",
        filePath: "Multiple files",
        unifiedDiff: "@@ -1 +1 @@\n-old\n+new",
      }),
    ).toBeNull();
    expect(extractToolCallFilePath({ type: "edit", filePath: "/repo/Multiple files" })).toBe(
      "/repo/Multiple files",
    );
  });

  it.each([
    ["cat ~/file.md", "~/file.md"],
    [
      "wc -l ~/.ait-server/plans/projects-settings-page.md",
      "~/.ait-server/plans/projects-settings-page.md",
    ],
    ["head -n 20 src/index.ts", "src/index.ts"],
    ["tail -f /var/log/x.log", "/var/log/x.log"],
    ["less ./README.md", "./README.md"],
    ["stat /tmp/foo", "/tmp/foo"],
  ])("matches shell command %s", (command, expected) => {
    expect(extractToolCallFilePath({ type: "shell", command })).toBe(expected);
  });

  it.each([
    "cat a.ts | grep foo",
    "cat a.ts > b.ts",
    "cat a.ts b.ts",
    "echo hi",
    "ls /tmp",
    "rm -rf /tmp",
  ])("returns null for non-matching shell %s", (command) => {
    expect(extractToolCallFilePath({ type: "shell", command })).toBeNull();
  });

  it("returns null for unrelated detail types", () => {
    expect(extractToolCallFilePath({ type: "search", query: "foo" })).toBeNull();
    expect(extractToolCallFilePath({ type: "unknown", input: null, output: null })).toBeNull();
    expect(extractToolCallFilePath(undefined)).toBeNull();
  });
});
