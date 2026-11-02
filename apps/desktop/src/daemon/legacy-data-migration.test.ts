import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  findLegacyDataMigrationCandidate,
  inspectDataHome,
  LegacyDataMigrationCancelledError,
  prepareLegacyDataMigration,
} from "./legacy-data-migration";

const roots: string[] = [];

function fixture(): { source: string; destination: string } {
  const root = mkdtempSync(path.join(tmpdir(), "ait legacy migration "));
  roots.push(root);
  return { source: path.join(root, ".ait-server"), destination: path.join(root, "desktop") };
}

function writeRegistry(home: string, relative: string, length: number): void {
  const file = path.join(home, relative);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(
    file,
    JSON.stringify(Array.from({ length }, (_value, index) => ({ id: `record-${index}` }))),
  );
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe("legacy data migration preparation", () => {
  it("detects populated legacy data beside an empty desktop destination", async () => {
    const { source, destination } = fixture();
    writeRegistry(source, "agents/agents.json", 4);
    writeRegistry(source, "projects/projects.json", 2);
    writeRegistry(source, "projects/workspaces.json", 3);
    mkdirSync(path.join(source, "agents"), { recursive: true });
    writeFileSync(path.join(source, "agents/timeline.sqlite3"), Buffer.alloc(128));

    const candidate = await findLegacyDataMigrationCandidate(source, destination);

    expect(candidate).toEqual({
      source: { home: source, agents: 4, projects: 2, workspaces: 3, timelineBytes: 128 },
      destination: {
        home: destination,
        agents: 0,
        projects: 0,
        workspaces: 0,
        timelineBytes: 0,
      },
    });
  });

  it("does not offer migration when the desktop destination already has records", async () => {
    const { source, destination } = fixture();
    mkdirSync(path.join(source, "agents"), { recursive: true });
    writeFileSync(path.join(source, "agents/agents.json"), "not-json");
    writeRegistry(destination, "projects/workspaces.json", 1);

    await expect(findLegacyDataMigrationCandidate(source, destination)).resolves.toBeNull();
  });

  it("copies only after confirmation and passes both explicit homes to the executor", async () => {
    const { source, destination } = fixture();
    writeRegistry(source, "agents/agents.json", 1);
    const migrate = vi.fn(async () => {});
    const confirm = vi.fn(async () => true);
    const beforeMigrate = vi.fn(async () => {});

    await prepareLegacyDataMigration({
      sourceHome: source,
      destinationHome: destination,
      binary: "/app/daemon",
      confirm,
      beforeMigrate,
      migrate,
    });

    expect(confirm).toHaveBeenCalledOnce();
    expect(beforeMigrate).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ source: expect.objectContaining({ home: source }) }),
    );
    expect(confirm.mock.invocationCallOrder[0]).toBeLessThan(
      beforeMigrate.mock.invocationCallOrder[0],
    );
    expect(beforeMigrate.mock.invocationCallOrder[0]).toBeLessThan(
      migrate.mock.invocationCallOrder[0],
    );
    expect(migrate).toHaveBeenCalledExactlyOnceWith("/app/daemon", source, destination);
  });

  it("recovers an interrupted confirmed migration before inspecting the destination", async () => {
    const { source, destination } = fixture();
    mkdirSync(destination, { recursive: true });
    writeFileSync(path.join(destination, ".legacy-data-migration.json"), "{}");
    const migrate = vi.fn(async () => {
      rmSync(path.join(destination, ".legacy-data-migration.json"));
      writeRegistry(destination, "agents/agents.json", 1);
    });
    const confirm = vi.fn(async () => true);

    await prepareLegacyDataMigration({
      sourceHome: source,
      destinationHome: destination,
      binary: "/app/daemon",
      confirm,
      migrate,
    });

    expect(migrate).toHaveBeenCalledExactlyOnceWith("/app/daemon", source, destination);
    expect(confirm).not.toHaveBeenCalled();
  });

  it("leaves both homes untouched when the user postpones migration", async () => {
    const { source, destination } = fixture();
    writeRegistry(source, "agents/agents.json", 1);
    const migrate = vi.fn(async () => {});
    const beforeMigrate = vi.fn(async () => {});

    await expect(
      prepareLegacyDataMigration({
        sourceHome: source,
        destinationHome: destination,
        binary: "/app/daemon",
        confirm: async () => false,
        beforeMigrate,
        migrate,
      }),
    ).rejects.toBeInstanceOf(LegacyDataMigrationCancelledError);
    expect(beforeMigrate).not.toHaveBeenCalled();
    expect(migrate).not.toHaveBeenCalled();
    await expect(inspectDataHome(destination)).resolves.toMatchObject({ agents: 0 });
  });
});
