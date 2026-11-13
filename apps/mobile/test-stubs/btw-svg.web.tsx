import React from "react";

// Bridge NativeSvg's primitive nodes to the browser without loading native Fabric.
const shape = (tag: string) =>
  React.forwardRef<SVGElement, Record<string, unknown>>((props, ref) =>
    React.createElement(tag, { ...props, ref }),
  );

export * from "./react-native-svg";
export const Svg = shape("svg");
export default Svg;
export const Circle = shape("circle");
export const Defs = shape("defs");
export const G = shape("g");
export const Line = shape("line");
export const LinearGradient = shape("linearGradient");
export const Path = shape("path");
export const Polygon = shape("polygon");
export const Polyline = shape("polyline");
export const Rect = shape("rect");
export const Stop = shape("stop");
export const Use = shape("use");
