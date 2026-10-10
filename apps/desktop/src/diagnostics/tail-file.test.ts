import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { tailFile } from "./tail-file";

let testDir = "";

describe("tailFile", () => {
  beforeEach(() => {
    testDir = mkdtempSync(path.join(tmpdir(), "paseo-tail-file-"));
  });

  afterEach(() => {
    rmSync(testDir, { recursive: true, force: true });
  });

  it("returns the requested tail lines", () => {
    const filePath = path.join(testDir, "main.log");
    writeFileSync(filePath, "one\ntwo\nthree\n");

    expect(tailFile(filePath, 2)).toBe("two\nthree");
  });

  it("keeps missing files empty", () => {
    expect(tailFile(path.join(testDir, "missing.log"), 2)).toBe("");
    expect(tailFile(path.join(testDir, "missing.log"), 2, { throwOnReadError: true })).toBe("");
  });

  it("can propagate read failures", () => {
    expect(() => tailFile(testDir, 2, { throwOnReadError: true })).toThrow();
    expect(tailFile(testDir, 2)).toBe("");
  });

  it("reads only the end of large files and drops the partial first line", () => {
    const filePath = path.join(testDir, "daemon.log");
    const filler = `${"x".repeat(1023)}\n`.repeat(1024);
    writeFileSync(filePath, `${filler}second-last\nlast\n`);

    expect(tailFile(filePath, 2)).toBe("second-last\nlast");
    const tail = tailFile(filePath, 10_000).split("\n");
    expect(tail.length).toBeLessThan(1024);
    expect(tail.every((line) => line === "x".repeat(1023) || line.endsWith("last"))).toBe(true);
  });
});
