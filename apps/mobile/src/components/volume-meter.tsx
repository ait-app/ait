import { useEffect, useMemo } from "react";
import { View } from "react-native";
import ReanimatedAnimated, {
  useSharedValue,
  useAnimatedStyle,
  withTiming,
  withRepeat,
  withSequence,
  Easing,
} from "react-native-reanimated";
import { StyleSheet, useUnistyles } from "react-native-unistyles";

interface VolumeMeterProps {
  volume: number;
  isMuted?: boolean;
  isSpeaking?: boolean;
  color?: string;
}

const LINE_SPACING = 8;
const LINE_WIDTH = 8;
const MAX_HEIGHT = 30;
const MIN_HEIGHT = 12;
const spacerStyle = { width: LINE_SPACING };

export function VolumeMeter({
  volume,
  isMuted = false,
  isSpeaking = false,
  color,
}: VolumeMeterProps) {
  const { theme } = useUnistyles();

  // Create shared values for 3 dots unconditionally
  const animatedVolume = useSharedValue(0);
  const line1Pulse = useSharedValue(1);
  const line2Pulse = useSharedValue(1);
  const line3Pulse = useSharedValue(1);

  // Start idle animations with different phases for all dots
  useEffect(() => {
    if (isMuted) {
      // When muted, set all pulses to 1 (no animation)
      line1Pulse.value = 1;
      line2Pulse.value = 1;
      line3Pulse.value = 1;
      return;
    }

    // Animate each dot with different phases and durations
    line1Pulse.value = withRepeat(
      withSequence(
        withTiming(1, { duration: 0 }),
        withTiming(1.15, { duration: 800, easing: Easing.inOut(Easing.ease) }),
        withTiming(1, { duration: 800, easing: Easing.inOut(Easing.ease) }),
      ),
      -1,
      false,
    );

    line2Pulse.value = withRepeat(
      withSequence(
        withTiming(1, { duration: 200 }),
        withTiming(1.2, { duration: 1000, easing: Easing.inOut(Easing.ease) }),
        withTiming(1, { duration: 1000, easing: Easing.inOut(Easing.ease) }),
      ),
      -1,
      false,
    );

    line3Pulse.value = withRepeat(
      withSequence(
        withTiming(1, { duration: 400 }),
        withTiming(1.25, { duration: 1200, easing: Easing.inOut(Easing.ease) }),
        withTiming(1, { duration: 1200, easing: Easing.inOut(Easing.ease) }),
      ),
      -1,
      false,
    );
  }, [isMuted, line1Pulse, line2Pulse, line3Pulse]);

  // Drive a single animated volume value and derive the individual bar heights
  // on the UI thread instead of scheduling three independent springs per sample.
  useEffect(() => {
    if (isMuted) {
      animatedVolume.value = 0;
      return;
    }

    animatedVolume.value = withTiming(volume, {
      duration: volume > animatedVolume.value ? 70 : 140,
      easing: Easing.out(Easing.cubic),
    });
  }, [animatedVolume, isMuted, volume]);

  const lineColor = color ?? theme.colors.foreground;

  // Create animated styles unconditionally at top level
  const line1Style = useAnimatedStyle(() => {
    const isActive = isSpeaking;
    let baseOpacity: number;
    if (isMuted) baseOpacity = 0.3;
    else if (isActive) baseOpacity = 0.9;
    else baseOpacity = 0.5;
    const currentVolume = isMuted ? 0 : animatedVolume.value;
    const currentHeight = MIN_HEIGHT + MAX_HEIGHT * currentVolume * 1.2;
    const volumeBoost = isMuted || !isActive ? 0 : currentVolume * 0.3;
    return {
      height: currentHeight * (isMuted || currentVolume > 0.001 ? 1 : line1Pulse.value),
      opacity: baseOpacity + volumeBoost,
    };
  });

  const line2Style = useAnimatedStyle(() => {
    const isActive = isSpeaking;
    let baseOpacity: number;
    if (isMuted) baseOpacity = 0.3;
    else if (isActive) baseOpacity = 0.9;
    else baseOpacity = 0.5;
    const currentVolume = isMuted ? 0 : animatedVolume.value;
    const currentHeight = MIN_HEIGHT + MAX_HEIGHT * currentVolume * 1.05;
    const volumeBoost = isMuted || !isActive ? 0 : currentVolume * 0.3;
    return {
      height: currentHeight * (isMuted || currentVolume > 0.001 ? 1 : line2Pulse.value),
      opacity: baseOpacity + volumeBoost,
    };
  });

  const line3Style = useAnimatedStyle(() => {
    const isActive = isSpeaking;
    let baseOpacity: number;
    if (isMuted) baseOpacity = 0.3;
    else if (isActive) baseOpacity = 0.9;
    else baseOpacity = 0.5;
    const currentVolume = isMuted ? 0 : animatedVolume.value;
    const currentHeight = MIN_HEIGHT + MAX_HEIGHT * currentVolume * 0.9;
    const volumeBoost = isMuted || !isActive ? 0 : currentVolume * 0.3;
    return {
      height: currentHeight * (isMuted || currentVolume > 0.001 ? 1 : line3Pulse.value),
      opacity: baseOpacity + volumeBoost,
    };
  });

  const lineBase = useMemo(() => ({ width: LINE_WIDTH, backgroundColor: lineColor }), [lineColor]);
  const line1CombinedStyle = useMemo(
    () => [styles.line, lineBase, line1Style],
    [lineBase, line1Style],
  );
  const line2CombinedStyle = useMemo(
    () => [styles.line, lineBase, line2Style],
    [lineBase, line2Style],
  );
  const line3CombinedStyle = useMemo(
    () => [styles.line, lineBase, line3Style],
    [lineBase, line3Style],
  );

  return (
    <View style={styles.container}>
      <ReanimatedAnimated.View style={line1CombinedStyle} />
      <View style={spacerStyle} />
      <ReanimatedAnimated.View style={line2CombinedStyle} />
      <View style={spacerStyle} />
      <ReanimatedAnimated.View style={line3CombinedStyle} />
    </View>
  );
}

const styles = StyleSheet.create((theme) => ({
  container: {
    height: 60,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
  },
  line: {
    borderRadius: theme.borderRadius.full,
  },
}));
