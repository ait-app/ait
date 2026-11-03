import { createPaseoClient, type PaseoClient, type PaseoClientConfig } from "@ait/client";

export function createClient(config: PaseoClientConfig): PaseoClient {
  return createPaseoClient(config);
}

export async function createOpenAndArchiveWorkspace(
  config: PaseoClientConfig,
  cwd: string,
): Promise<void> {
  const client = createClient(config);

  try {
    await client.connect();

    const created = await client.workspaces.create({
      source: { kind: "directory", path: cwd },
      title: "Fresh SDK workspace",
    });
    const opened = await client.workspaces.open(cwd);

    await opened.refresh();
    await created.archive();
  } finally {
    await client.close();
  }
}
