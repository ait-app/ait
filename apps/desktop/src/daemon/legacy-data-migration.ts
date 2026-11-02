import { execFile } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

const MIGRATION_JOURNAL_FILENAME = ".legacy-data-migration.json";

export interface DataHomeSummary {
  home: string;
  agents: number;
  projects: number;
  workspaces: number;
  timelineBytes: number;
}

export interface LegacyDataMigrationCandidate {
  source: DataHomeSummary;
  destination: DataHomeSummary;
}

interface PrepareLegacyDataMigrationOptions {
  sourceHome: string;
  destinationHome: string;
  binary: string;
  confirm: (candidate: LegacyDataMigrationCandidate) => Promise<boolean>;
  migrate?: (binary: string, source: string, destination: string) => Promise<void>;
}

export class LegacyDataMigrationCancelledError extends Error {
  constructor() {
    super("Existing Ait data was not copied. The empty desktop Host was not started.");
    this.name = "LegacyDataMigrationCancelledError";
  }
}

/** Inspect the durable registries without creating or modifying the selected data directory. */
export async function inspectDataHome(home: string): Promise<DataHomeSummary> {
  const [agents, projects, workspaces, timelineBytes] = await Promise.all([
    readJsonArrayLength(path.join(home, "agents/agents.json")),
    readJsonArrayLength(path.join(home, "projects/projects.json")),
    readJsonArrayLength(path.join(home, "projects/workspaces.json")),
    readFileSize(path.join(home, "agents/timeline.sqlite3")),
  ]);
  return { home, agents, projects, workspaces, timelineBytes };
}

/** Detect a populated legacy home only when the desktop destination has no durable records. */
export async function findLegacyDataMigrationCandidate(
  sourceHome: string,
  destinationHome: string,
): Promise<LegacyDataMigrationCandidate | null> {
  if (path.resolve(sourceHome) === path.resolve(destinationHome)) return null;
  const destination = await inspectDataHome(destinationHome);
  const destinationHasData =
    destination.agents > 0 || destination.projects > 0 || destination.workspaces > 0;
  if (destinationHasData) return null;
  const source = await inspectDataHome(sourceHome);
  const sourceHasData = source.agents > 0 || source.projects > 0 || source.workspaces > 0;
  return sourceHasData && !destinationHasData ? { source, destination } : null;
}

/** Prompt for and execute the one-time copy before an empty desktop daemon can start. */
export async function prepareLegacyDataMigration(
  options: PrepareLegacyDataMigrationOptions,
): Promise<void> {
  const migrate = options.migrate ?? runLegacyDataMigration;
  if (await fileExists(path.join(options.destinationHome, MIGRATION_JOURNAL_FILENAME))) {
    await migrate(options.binary, options.sourceHome, options.destinationHome);
  }
  const candidate = await findLegacyDataMigrationCandidate(
    options.sourceHome,
    options.destinationHome,
  );
  if (!candidate) return;
  if (!(await options.confirm(candidate))) throw new LegacyDataMigrationCancelledError();
  await migrate(options.binary, candidate.source.home, candidate.destination.home);
}

/** Run the daemon-owned exclusive migration command and surface its bounded diagnostics. */
export function runLegacyDataMigration(
  binary: string,
  source: string,
  destination: string,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const env = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !key.startsWith("AIT_SERVER_")),
    );
    execFile(
      binary,
      ["migrate-data", "--source", source, "--data-dir", destination, "--json"],
      { env, timeout: 10 * 60_000, maxBuffer: 1024 * 1024, windowsHide: true },
      (error, _stdout, stderr) => {
        if (!error) {
          resolve();
          return;
        }
        const detail = stderr.trim();
        reject(
          new Error(
            detail
              ? `Ait could not copy the existing data: ${detail}`
              : `Ait could not copy the existing data: ${error.message}`,
          ),
        );
      },
    );
  });
}

async function readJsonArrayLength(file: string): Promise<number> {
  let contents: string;
  try {
    contents = await readFile(file, "utf8");
  } catch (error) {
    if (isMissing(error)) return 0;
    throw error;
  }
  const value: unknown = JSON.parse(contents);
  if (!Array.isArray(value)) throw new Error(`Ait data registry is not an array: ${file}`);
  return value.length;
}

async function readFileSize(file: string): Promise<number> {
  try {
    return (await stat(file)).size;
  } catch (error) {
    if (isMissing(error)) return 0;
    throw error;
  }
}

async function fileExists(file: string): Promise<boolean> {
  try {
    await stat(file);
    return true;
  } catch (error) {
    if (isMissing(error)) return false;
    throw error;
  }
}

function isMissing(error: unknown): boolean {
  return (
    error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT"
  );
}
