import { describe, expect, it } from "vitest";
import {
  buildAbsoluteExplorerPath,
  pruneDeletedExplorerEntry,
  pruneExplorerPathsWithin,
} from "./explorer-paths";

describe("buildAbsoluteExplorerPath", () => {
  it("builds a POSIX absolute path from a relative explorer path", () => {
    expect(
      buildAbsoluteExplorerPath({
        workspaceRoot: "/workspaces/paseo",
        entryPath: "packages/app/src/components/file-explorer-pane.tsx",
      }),
    ).toBe("/workspaces/paseo/packages/app/src/components/file-explorer-pane.tsx");
  });

  it("returns workspace root when entry path points to explorer root", () => {
    expect(
      buildAbsoluteExplorerPath({
        workspaceRoot: "/workspaces/paseo",
        entryPath: ".",
      }),
    ).toBe("/workspaces/paseo");
  });

  it("trims trailing separators from workspace root before joining", () => {
    expect(
      buildAbsoluteExplorerPath({
        workspaceRoot: "/workspaces/paseo/",
        entryPath: "README.md",
      }),
    ).toBe("/workspaces/paseo/README.md");
  });

  it("builds a Windows absolute path with backslash separators", () => {
    expect(
      buildAbsoluteExplorerPath({
        workspaceRoot: "C:\\repo\\paseo",
        entryPath: "packages/app/src/components/file-explorer-pane.tsx",
      }),
    ).toBe("C:\\repo\\paseo\\packages\\app\\src\\components\\file-explorer-pane.tsx");
  });

  it("passes through an already-absolute entry path", () => {
    expect(
      buildAbsoluteExplorerPath({
        workspaceRoot: "/workspaces/paseo",
        entryPath: "/tmp/another/location.txt",
      }),
    ).toBe("/tmp/another/location.txt");
  });

  it("joins relative paths under a filesystem root workspace", () => {
    expect(buildAbsoluteExplorerPath({ workspaceRoot: "/", entryPath: "src/a.ts" })).toBe(
      "/src/a.ts",
    );
  });

  it.each(["", "."])("returns the filesystem root for entry path %j", (entryPath) => {
    expect(buildAbsoluteExplorerPath({ workspaceRoot: "/", entryPath })).toBe("/");
  });
});

describe("pruneExplorerPathsWithin", () => {
  it("removes the deleted folder and its descendants but not name-prefixed siblings", () => {
    expect(pruneExplorerPathsWithin([".", "src", "src/a", "src-old"], "src")).toEqual([
      ".",
      "src-old",
    ]);
  });
});

describe("pruneDeletedExplorerEntry", () => {
  const directory = (path: string) => ({ path, entries: [] });
  const directories = new Map([".", "src", "src/a", "src-old"].map((p) => [p, directory(p)]));

  it("drops cached listings for the deleted folder and its descendants", () => {
    const pruned = pruneDeletedExplorerEntry({ directories, selectedEntryPath: null }, "src");

    expect(Array.from(pruned.directories.keys())).toEqual([".", "src-old"]);
  });

  it("clears a selection inside the deleted folder", () => {
    expect(
      pruneDeletedExplorerEntry({ directories, selectedEntryPath: "src/a/file.ts" }, "src")
        .selectedEntryPath,
    ).toBeNull();
  });

  it("keeps a selection outside the deleted folder", () => {
    expect(
      pruneDeletedExplorerEntry({ directories, selectedEntryPath: "src-old/file.ts" }, "src")
        .selectedEntryPath,
    ).toBe("src-old/file.ts");
  });
});
