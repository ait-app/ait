import { describe, expect, it } from "vitest";
import { isAbsolutePath, isHomeRelativePath, normalizeCwd } from "./path";

describe("isAbsolutePath", () => {
  it("returns true for Unix absolute paths", () => {
    expect(isAbsolutePath("/")).toBe(true);
    expect(isAbsolutePath("/home/user")).toBe(true);
    expect(isAbsolutePath("/tmp/file.txt")).toBe(true);
  });

  it("returns true for Windows drive letter paths", () => {
    expect(isAbsolutePath("C:\\Users")).toBe(true);
    expect(isAbsolutePath("C:/Users")).toBe(true);
    expect(isAbsolutePath("d:\\projects")).toBe(true);
  });

  it("returns true for UNC paths", () => {
    expect(isAbsolutePath("\\\\server\\share")).toBe(true);
    expect(isAbsolutePath("\\\\\\\\host\\path")).toBe(true);
  });

  it("returns false for relative paths", () => {
    expect(isAbsolutePath("foo/bar")).toBe(false);
    expect(isAbsolutePath("./relative")).toBe(false);
    expect(isAbsolutePath("../parent")).toBe(false);
    expect(isAbsolutePath("")).toBe(false);
    expect(isAbsolutePath("file.txt")).toBe(false);
  });

  it("returns false for edge cases that are not absolute paths", () => {
    expect(isAbsolutePath("")).toBe(false);
    expect(isAbsolutePath("C:")).toBe(false);
  });

  it("handles mixed separators in absolute paths", () => {
    expect(isAbsolutePath("C:/Users\\mixed/path")).toBe(true);
    expect(isAbsolutePath("/tmp\\mixed/path")).toBe(true);
  });
});

describe("isHomeRelativePath", () => {
  it("returns true for the home directory and paths under it", () => {
    expect(isHomeRelativePath("~")).toBe(true);
    expect(isHomeRelativePath("~/notes.md")).toBe(true);
    expect(isHomeRelativePath("~\\notes.md")).toBe(true);
  });

  it("returns false for other paths", () => {
    expect(isHomeRelativePath("")).toBe(false);
    expect(isHomeRelativePath("~user/notes.md")).toBe(false);
    expect(isHomeRelativePath("/home/user")).toBe(false);
    expect(isHomeRelativePath("docs/~/notes.md")).toBe(false);
  });
});

describe("normalizeCwd", () => {
  it("trims whitespace and trailing slashes", () => {
    expect(normalizeCwd("  /repo/app  ")).toBe("/repo/app");
    expect(normalizeCwd("/repo/app/")).toBe("/repo/app");
    expect(normalizeCwd("/repo/app///")).toBe("/repo/app");
  });

  it("keeps a bare root", () => {
    expect(normalizeCwd("/")).toBe("/");
    expect(normalizeCwd(" / ")).toBe("/");
  });

  it("leaves backslash separators alone", () => {
    expect(normalizeCwd("C:\\repo\\")).toBe("C:\\repo\\");
  });
});
