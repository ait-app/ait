// @vitest-environment jsdom
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { WorkspaceHeaderBreadcrumb } from "./workspace-header-breadcrumb";

beforeEach(() => vi.stubGlobal("React", React));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

describe("workspace header breadcrumb", () => {
  it("leads from the project to the workspace with a separator between them", () => {
    render(<WorkspaceHeaderBreadcrumb project="ait" workspace="fix-login" showProject />);

    const project = screen.getByTestId("workspace-header-subtitle");
    const separator = screen.getByTestId("workspace-header-breadcrumb-separator");
    const workspace = screen.getByTestId("workspace-header-title");
    expect(project.textContent).toBe("ait");
    expect(workspace.textContent).toBe("fix-login");
    expect(project.compareDocumentPosition(separator)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(separator.compareDocumentPosition(workspace)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  });

  it("shows only the workspace when the project name would repeat it", () => {
    render(<WorkspaceHeaderBreadcrumb project="ait" workspace="ait" showProject={false} />);

    expect(screen.queryByTestId("workspace-header-subtitle")).toBeNull();
    expect(screen.queryByTestId("workspace-header-breadcrumb-separator")).toBeNull();
    expect(screen.getByTestId("workspace-header-title").textContent).toBe("ait");
  });
});
