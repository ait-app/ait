import Svg, { G, Path } from "react-native-svg";
import { useUnistyles } from "react-native-unistyles";
import brand from "../../../../../assets/brand/ait-mark.json";

interface AitLogoProps {
  size?: number;
  color?: string;
  variant?: "mark" | "stacked";
  wordmarkColor?: string;
}

// The asset generator consumes this same geometry; never duplicate paths in a screen.
export function AitLogo({ size = 64, color, variant = "mark", wordmarkColor }: AitLogoProps) {
  const { theme } = useUnistyles();
  const colors = theme.colorScheme === "dark" ? brand.darkColors : brand.colors;
  const stacked = variant === "stacked";
  return (
    <Svg width={size} height={size} viewBox="0 0 1024 1024" fill="none" accessibilityLabel="AIT">
      <G
        transform={
          stacked
            ? brand.placements.stackedMark
            : size < 32
              ? brand.placements.smallIcon
              : brand.placements.icon
        }
      >
        {color !== undefined || size < 32 ? (
          <Path
            d={size < 32 ? brand.smallSilhouette : brand.silhouette}
            fill={color ?? colors.primary}
          />
        ) : (
          <>
            <Path d={brand.faces[0]} fill={colors.primary} />
            <Path d={brand.faces[1]} fill={colors.secondary} />
          </>
        )}
      </G>
      {stacked && (
        <Path
          d={brand.wordmark}
          transform={brand.placements.stackedWordmark}
          fill={wordmarkColor ?? color ?? colors.ink}
        />
      )}
    </Svg>
  );
}
