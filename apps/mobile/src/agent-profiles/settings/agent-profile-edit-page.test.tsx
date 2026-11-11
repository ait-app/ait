/** @vitest-environment jsdom */
import type { ProviderSnapshotEntry } from "@ait/protocol/agent-types";
import React, { useEffect } from "react";
import type { AgentProfile } from "@ait/protocol/messages";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { i18n } from "@/i18n/i18next";
import { AgentProfileEditPage } from "./agent-profile-edit-page";

const store = vi.hoisted(() => ({
  profiles: null as AgentProfile[] | null,
  isSupported: true,
  saveProfiles: vi.fn(async (_next: AgentProfile[]) => {}),
  entries: undefined as ProviderSnapshotEntry[] | undefined,
}));

vi.mock("@/constants/layout", () => ({ useIsCompactFormFactor: () => false }));
vi.mock("../internal/use-agent-profiles", () => ({
  useAgentProfiles: () => ({
    profiles: store.profiles,
    isSupported: store.isSupported,
    saveProfiles: store.saveProfiles,
  }),
}));
vi.mock("../internal/use-profile-form-inputs", () => ({
  useAgentProfileFormCatalog: ({
    model,
  }: {
    model: { applyProviderCatalog: (e: ProviderSnapshotEntry[]) => void };
  }) => {
    // Mirror the real hook: the catalog lands after mount, not mid-render.
    useEffect(() => {
      if (store.entries) {
        model.applyProviderCatalog(store.entries);
      }
    }, [model]);
    return { entries: store.entries, error: null, refetchIfStale: vi.fn() };
  },
  useAgentProfileFormFeatures: () => {},
}));

const CLAUDE: ProviderSnapshotEntry = {
  provider: "claude",
  status: "ready",
  enabled: true,
  label: "Claude Code",
  defaultModeId: "plan",
  modes: [{ id: "plan", label: "Plan" }],
  models: [
    {
      provider: "claude",
      id: "claude-opus-5",
      label: "Opus 5",
      isDefault: true,
      thinkingOptions: [{ id: "think", label: "Think" }],
    },
  ],
};

const STORED: AgentProfile = {
  id: "p1",
  name: "UI work",
  provider: "claude",
  model: "claude-opus-5",
  // Passthrough data a newer client wrote; this editor must not interpret it.
  futureSetting: { nested: ["keep", 42] },
};

beforeEach(async () => {
  vi.stubGlobal("React", React);
  await i18n.changeLanguage("en");
  store.profiles = [{ ...STORED }];
  store.isSupported = true;
  store.entries = [CLAUDE];
  store.saveProfiles.mockClear();
});
afterEach(cleanup);

function renderEdit(profileId: string | null = "p1", onDone = vi.fn()) {
  const view = render(<AgentProfileEditPage serverId="h1" profileId={profileId} onDone={onDone} />);
  return { ...view, onDone };
}

describe("AgentProfileEditPage", () => {
  it("keeps passthrough fields when a save clears an owned field", async () => {
    const { getByTestId } = renderEdit();
    await waitFor(() => expect(getByTestId("agent-profile-edit-page")).toBeTruthy());

    // Clear the stored model through the explicit blank row.
    fireEvent.click(getByTestId("agent-profile-model-trigger"));
    fireEvent.click(await waitFor(() => getByTestId("agent-profile-leave-unchanged-option")));
    fireEvent.click(getByTestId("agent-profile-save-button"));

    await waitFor(() => expect(store.saveProfiles).toHaveBeenCalledTimes(1));
    const saved = store.saveProfiles.mock.calls[0][0] as AgentProfile[];
    expect(saved).toHaveLength(1);
    expect(saved[0].id).toBe("p1");
    expect(saved[0].model).toBeUndefined();
    expect(saved[0].futureSetting).toEqual({ nested: ["keep", 42] });
    expect(saved[0].name).toBe("UI work");
  });

  it("keeps other profiles untouched when editing one", async () => {
    const other: AgentProfile = { id: "p2", name: "Other", provider: "claude", otherFuture: true };
    store.profiles = [{ ...STORED }, other];
    const { getByTestId } = renderEdit();
    await waitFor(() => expect(getByTestId("agent-profile-edit-page")).toBeTruthy());

    fireEvent.change(getByTestId("agent-profile-name-input"), { target: { value: "Renamed" } });
    fireEvent.click(getByTestId("agent-profile-save-button"));

    await waitFor(() => expect(store.saveProfiles).toHaveBeenCalledTimes(1));
    const saved = store.saveProfiles.mock.calls[0][0] as AgentProfile[];
    expect(saved[1]).toEqual(other);
    expect(saved[0].name).toBe("Renamed");
    expect(saved[0].futureSetting).toEqual({ nested: ["keep", 42] });
  });

  it("reports a missing profile without writing", async () => {
    const { getByTestId } = renderEdit("does-not-exist");
    await waitFor(() => expect(getByTestId("agent-profile-edit-missing")).toBeTruthy());
    expect(getByTestId("agent-profile-edit-missing").textContent).toBe("Profile not found");
    expect(store.saveProfiles).not.toHaveBeenCalled();
  });

  it("waits for the config in both modes before rendering a form", () => {
    store.profiles = null;
    const edit = renderEdit("p1");
    expect(edit.queryByTestId("agent-profile-edit-page")).toBeNull();
    edit.unmount();
    const create = renderEdit(null);
    expect(create.queryByTestId("agent-profile-edit-page")).toBeNull();
    expect(store.saveProfiles).not.toHaveBeenCalled();
  });

  it("shows an unsupported host card instead of the form", () => {
    store.isSupported = false;
    const { getByTestId } = renderEdit();
    expect(getByTestId("agent-profile-edit-unavailable")).toBeTruthy();
  });

  it("marks a stored model the catalog does not offer as unresolved and keeps it on save", async () => {
    store.profiles = [{ ...STORED, model: "claude-retired-3" }];
    const { getByTestId } = renderEdit();
    await waitFor(() => expect(getByTestId("agent-profile-edit-page")).toBeTruthy());

    // The trigger reads the raw id and says the host does not offer it.
    const trigger = getByTestId("agent-profile-model-trigger");
    await waitFor(() =>
      expect(trigger.getAttribute("aria-label")).toBe("Model (claude-retired-3)"),
    );
    expect(getByTestId("agent-profile-model-field").textContent).toContain(
      "Not offered by this host",
    );

    // The stored id has its own option row, so opening the field cannot land
    // on the blank row and erase the value with one keystroke.
    fireEvent.click(trigger);
    const keptOption = await waitFor(() => getByTestId("agent-profile-unresolved-option"));
    expect(keptOption.textContent).toContain("claude-retired-3");
    fireEvent.click(keptOption);
    fireEvent.click(getByTestId("agent-profile-save-button"));

    await waitFor(() => expect(store.saveProfiles).toHaveBeenCalledTimes(1));
    const saved = store.saveProfiles.mock.calls[0][0] as AgentProfile[];
    expect(saved[0].model).toBe("claude-retired-3");
    expect(saved[0].futureSetting).toEqual({ nested: ["keep", 42] });
  });

  it("offers leave-unchanged rows for omitted fields and keeps them omitted on save", async () => {
    store.profiles = [{ id: "p1", name: "Bare", provider: "claude" }];
    const { getByTestId } = renderEdit();
    await waitFor(() => expect(getByTestId("agent-profile-edit-page")).toBeTruthy());

    fireEvent.click(getByTestId("agent-profile-model-trigger"));
    const unset = await waitFor(() => getByTestId("agent-profile-leave-unchanged-option"));
    expect(unset.textContent).toContain("Leave unchanged");
    fireEvent.click(unset);
    fireEvent.click(getByTestId("agent-profile-save-button"));

    await waitFor(() => expect(store.saveProfiles).toHaveBeenCalledTimes(1));
    const saved = store.saveProfiles.mock.calls[0][0] as AgentProfile[];
    expect(saved[0].model).toBeUndefined();
    expect(saved[0].modeId).toBeUndefined();
    expect(saved[0].thinkingOptionId).toBeUndefined();
  });
});
