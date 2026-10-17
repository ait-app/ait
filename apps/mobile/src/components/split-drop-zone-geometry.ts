export type SplitDropZonePosition = "center" | "left" | "right" | "top" | "bottom";

export interface SplitDropZoneHover {
  paneId: string;
  position: SplitDropZonePosition;
}

const EDGE_RATIO = 0.15;
const CENTER_RATIO = 0.4;

export function resolveSplitDropPosition(input: {
  width: number;
  height: number;
  x: number;
  y: number;
}): SplitDropZonePosition {
  const centerInsetX = input.width * ((1 - CENTER_RATIO) / 2);
  const centerInsetY = input.height * ((1 - CENTER_RATIO) / 2);
  const insideCenterX = input.x >= centerInsetX && input.x <= input.width - centerInsetX;
  const insideCenterY = input.y >= centerInsetY && input.y <= input.height - centerInsetY;

  if (insideCenterX && insideCenterY) {
    return "center";
  }

  const edgeThresholdX = input.width * EDGE_RATIO;
  const edgeThresholdY = input.height * EDGE_RATIO;
  if (input.x <= edgeThresholdX) {
    return "left";
  }
  if (input.x >= input.width - edgeThresholdX) {
    return "right";
  }
  if (input.y <= edgeThresholdY) {
    return "top";
  }
  if (input.y >= input.height - edgeThresholdY) {
    return "bottom";
  }

  const distances: {
    position: Exclude<SplitDropZonePosition, "center">;
    distance: number;
  }[] = [
    { position: "left", distance: input.x },
    { position: "right", distance: input.width - input.x },
    { position: "top", distance: input.y },
    { position: "bottom", distance: input.height - input.y },
  ];
  distances.sort((left, right) => left.distance - right.distance);
  return distances[0]?.position ?? "center";
}
