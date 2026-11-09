import React, { useMemo } from "react";
import { View } from "react-native";
import qrcode from "qrcode-generator";
import { SvgXml } from "react-native-svg";

const QUIET_ZONE_MODULES = 4;

/** SVG for `value`, always dark on light so phone cameras can read it in any app theme. */
function qrSvg(value: string): string {
  const code = qrcode(0, "M");
  code.addData(value);
  code.make();
  const modules = code.getModuleCount();
  const size = modules + QUIET_ZONE_MODULES * 2;
  let path = "";
  for (let row = 0; row < modules; row += 1) {
    for (let column = 0; column < modules; column += 1) {
      if (code.isDark(row, column)) {
        path += `M${column + QUIET_ZONE_MODULES} ${row + QUIET_ZONE_MODULES}h1v1h-1z`;
      }
    }
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${size} ${size}" shape-rendering="crispEdges"><rect width="${size}" height="${size}" fill="#ffffff"/><path d="${path}" fill="#000000"/></svg>`;
}

export function QrCode({
  value,
  size,
  accessibilityLabel,
  testID,
}: {
  value: string;
  size: number;
  accessibilityLabel: string;
  testID?: string;
}) {
  const xml = useMemo(() => qrSvg(value), [value]);
  return (
    <View
      accessible
      accessibilityRole="image"
      accessibilityLabel={accessibilityLabel}
      testID={testID}
    >
      <SvgXml xml={xml} width={size} height={size} />
    </View>
  );
}
