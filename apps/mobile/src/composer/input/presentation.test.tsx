/** @vitest-environment jsdom */
import React from "react";
import { cleanup, render, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Text } from "react-native";
import { ComposerInputPresentation } from "./presentation";

vi.mock("@/composer/dock", () => ({
  ComposerFullscreenBody: ({ children }: React.PropsWithChildren) => <>{children}</>,
  ComposerFullscreenFooter: ({ children }: React.PropsWithChildren) => <>{children}</>,
}));
beforeEach(() => vi.stubGlobal("React", React));
afterEach(cleanup);

it.each([false, true])(
  "keeps command guidance inside the input border before the toolbar (fullscreen=%s)",
  (fullscreen) => {
    const view = render(
      <ComposerInputPresentation
        fullscreen={fullscreen}
        sendIconSize={16}
        surface={<Text>/goal</Text>}
        inputHint={<Text testID="hint">[objective]</Text>}
        toolbar={<Text testID="toolbar">Toolbar</Text>}
        send={null}
        error={null}
        onExit={() => {}}
        inputWrapperStyle={{ borderWidth: 1 }}
        inputPointerEvents="auto"
        attachmentSlot={null}
        showFullscreenButton={false}
        onEnter={() => {}}
        overlay={null}
      />,
    );
    const surface = view.getByTestId("composer-input-surface");
    expect(within(surface).getByTestId("hint")).toBeTruthy();
    if (!fullscreen) expect(surface.textContent).toBe("/goal[objective]Toolbar");
  },
);
