import { describe, expect, it } from "vitest";
import { CreateAgentPreferencesService } from "./service";
import { FakeCreateAgentPreferenceStorage } from "./test-utils/fake-preference-storage";

class FlakyReadStorage extends FakeCreateAgentPreferenceStorage {
  failNextRead = true;

  override async read(): Promise<unknown> {
    if (this.failNextRead) {
      this.failNextRead = false;
      throw new Error("storage unavailable");
    }
    return super.read();
  }
}

describe("CreateAgentPreferencesService.load", () => {
  it("retries storage after a failed read instead of caching the rejection", async () => {
    const storage = new FlakyReadStorage({ stored: { isolation: "worktree" } });
    const preferences = new CreateAgentPreferencesService(storage);

    await expect(preferences.load()).rejects.toThrow("storage unavailable");
    await expect(preferences.load()).resolves.toMatchObject({ isolation: "worktree" });
  });
});
