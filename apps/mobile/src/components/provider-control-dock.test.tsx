// @vitest-environment jsdom
import React from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { ProviderControlDock } from "./provider-control-dock";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
afterEach(cleanup);

it("keeps current controls in one row while disclosures preserve input and native lifecycle", () => {
  const onStop = vi.fn();
  const items = [
    {
      id: "goal",
      label: "Goal · Active",
      content: (
        <div>
          <textarea defaultValue="Notes" />
          <button onClick={onStop}>Pause</button>
        </div>
      ),
    },
    { id: "btw", label: "BTW", content: <div>Answer</div> },
  ];
  render(<ProviderControlDock owner="collapse-test" items={items} initial="goal" />);
  const goal = screen.getByTestId("provider-control-toggle-goal");
  expect(goal.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(goal);
  expect(goal.getAttribute("aria-expanded")).toBe("false");
  expect(screen.getByTestId("provider-control-content-goal").style.display).toBe("none");
  expect(onStop).not.toHaveBeenCalled();
  fireEvent.click(screen.getByTestId("provider-control-toggle-btw"));
  expect(screen.getByTestId("provider-control-toggle-btw").getAttribute("aria-expanded")).toBe(
    "true",
  );
  fireEvent.click(goal);
  expect(screen.getByDisplayValue("Notes")).toBeTruthy();
  expect(
    screen.getByTestId("provider-control-row").querySelectorAll('[role="button"]'),
  ).toHaveLength(2);
});

it("retains disclosure across navigation, isolates owners, and drops removed controls", () => {
  const items = [{ id: "goal", label: "Goal · Paused", content: <div>Current objective</div> }];
  const view = render(<ProviderControlDock owner="owner-one" items={items} initial="goal" />);
  fireEvent.click(screen.getByTestId("provider-control-toggle-goal"));
  view.unmount();
  const restored = render(<ProviderControlDock owner="owner-one" items={items} initial="goal" />);
  expect(screen.getByTestId("provider-control-toggle-goal").getAttribute("aria-expanded")).toBe(
    "false",
  );
  restored.unmount();
  const other = render(<ProviderControlDock owner="owner-two" items={items} initial="goal" />);
  expect(screen.getByTestId("provider-control-toggle-goal").getAttribute("aria-expanded")).toBe(
    "true",
  );
  other.rerender(<ProviderControlDock owner="owner-two" items={[]} />);
  expect(screen.queryByTestId("provider-control-dock")).toBeNull();
});

it("reveals a newly requested BTW panel once and supports close/reopen without a second disclosure", () => {
  const items = [
    {
      id: "btw",
      label: "BTW",
      content: ({ collapse }: { collapse: () => void }) => (
        <div>
          <textarea defaultValue="Unsent BTW" />
          <button onClick={collapse}>Close BTW</button>
        </div>
      ),
    },
  ];
  const view = render(<ProviderControlDock owner="btw-reveal" items={items} />);
  const toggle = screen.getByTestId("provider-control-toggle-btw");
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  view.rerender(
    <ProviderControlDock
      owner="btw-reveal"
      items={items}
      reveal={{ id: "btw", revision: "first" }}
    />,
  );
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  fireEvent.click(screen.getByText("Close BTW"));
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  view.rerender(
    <ProviderControlDock
      owner="btw-reveal"
      items={items}
      reveal={{ id: "btw", revision: "first" }}
    />,
  );
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(screen.getByDisplayValue("Unsent BTW")).toBeTruthy();
  fireEvent.click(toggle);
  view.rerender(
    <ProviderControlDock
      owner="btw-reveal"
      items={items}
      reveal={{ id: "btw", revision: "second" }}
    />,
  );
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
});
