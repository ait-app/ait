import { Bot } from "lucide-react-native";
import { SvgXml } from "react-native-svg";
import { describe, expect, it } from "vitest";
import { replaceProviderSnapshotIcons } from "./provider-icon-name";
import { getProviderIcon, type ProviderIconComponent } from "./provider-icons";
import { ACP_PROVIDER_ICON_SVGS } from "@/assets/acp-provider-icons";

function renderIcon(Component: ProviderIconComponent) {
  if (typeof Component !== "function") throw new Error("Expected a function component");
  return (Component as (props: { size: number; color: string }) => unknown)({
    size: 18,
    color: "#123456",
  });
}

describe("getProviderIcon", () => {
  it("renders Antigravity with the existing AGY brand asset", () => {
    expect(renderIcon(getProviderIcon("antigravity"))).toMatchObject({
      type: SvgXml,
      props: { xml: ACP_PROVIDER_ICON_SVGS.agy, width: 18, height: 18, color: "#123456" },
    });
  });
  it("renders registered snapshot SVG metadata with the requested size and color", () => {
    const svg = '<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z" /></svg>';
    replaceProviderSnapshotIcons("server-1", [{ provider: "rendered-provider", iconSvg: svg }]);

    const rendered = renderIcon(getProviderIcon("rendered-provider", "server-1"));

    expect(rendered).toMatchObject({
      type: SvgXml,
      props: { xml: svg, width: 18, height: 18, color: "#123456" },
    });
  });

  it("uses the normal Bot fallback without snapshot SVG metadata", () => {
    replaceProviderSnapshotIcons("server-1", [{ provider: "plain-provider" }]);

    expect(getProviderIcon("plain-provider", "server-1")).toBe(Bot);
  });
});
