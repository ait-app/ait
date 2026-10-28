import path from "node:path";
import { expect, test } from "../support/fixtures";
import { gotoAppShell, openSettings } from "../support/helpers/app";
import { repoRoot } from "../support/helpers/ait-daemon";
import { getServerId } from "../support/helpers/server-id";
import {
  expectProviderInstalledInSettings,
  openSettingsHost,
  openSettingsHostSection,
} from "../support/helpers/settings";

test.use({
  e2eDaemonEnvironment: {
    AIT_SERVER_MINIMAX_CODE_BIN: path.join(
      repoRoot,
      "crates/provider/src/local/minimax/tests/fixtures/acp.py",
    ),
  },
});

test("shows built-in MiniMax Code without adding a custom ACP provider", async ({ page }) => {
  await gotoAppShell(page);
  await openSettings(page);
  await openSettingsHost(page, getServerId());
  await openSettingsHostSection(page, getServerId(), "providers");
  await expectProviderInstalledInSettings(page, "MiniMax Code");
  await expect(page.getByTestId("provider-actions-minimax-code")).toHaveCount(0);
});
