import type { ReactNode } from "react";
import { withUnistyles } from "react-native-unistyles";
import { CircleCheck, CircleDot, CircleSlash, CircleX } from "lucide-react-native";
import { ManualStatusIcon } from "@/components/icons/manual-status-icon";
import {
  mutedIconColorMapping,
  statusDangerIconColorMapping,
  statusSuccessIconColorMapping,
  statusWarningIconColorMapping,
} from "@/components/ui/icon-color";
import type { CheckPresentation } from "./check-presentation";

const ThemedCircleCheck = withUnistyles(CircleCheck);
const ThemedCircleDot = withUnistyles(CircleDot);
const ThemedCircleSlash = withUnistyles(CircleSlash);
const ThemedCircleX = withUnistyles(CircleX);
const ThemedManualStatusIcon = withUnistyles(ManualStatusIcon);

export type CheckPresentationTone = "success" | "danger" | "warning" | "muted";

interface CheckPresentationViewPolicy {
  tone: CheckPresentationTone;
  renderIcon: (size: number) => ReactNode;
}

const CHECK_PRESENTATION_VIEW_POLICY: Record<CheckPresentation, CheckPresentationViewPolicy> = {
  success: {
    tone: "success",
    renderIcon(size) {
      return <ThemedCircleCheck size={size} uniProps={statusSuccessIconColorMapping} />;
    },
  },
  failure: {
    tone: "danger",
    renderIcon(size) {
      return <ThemedCircleX size={size} uniProps={statusDangerIconColorMapping} />;
    },
  },
  warning: {
    tone: "warning",
    renderIcon(size) {
      return <ThemedCircleX size={size} uniProps={statusWarningIconColorMapping} />;
    },
  },
  actionRequired: {
    tone: "warning",
    renderIcon(size) {
      return <ThemedManualStatusIcon size={size} uniProps={statusWarningIconColorMapping} />;
    },
  },
  manual: {
    tone: "muted",
    renderIcon(size) {
      return <ThemedManualStatusIcon size={size} uniProps={mutedIconColorMapping} />;
    },
  },
  pending: {
    tone: "warning",
    renderIcon(size) {
      return <ThemedCircleDot size={size} uniProps={statusWarningIconColorMapping} />;
    },
  },
  ignored: {
    tone: "muted",
    renderIcon(size) {
      return <ThemedCircleSlash size={size} uniProps={mutedIconColorMapping} />;
    },
  },
};

export function getCheckPresentationTone(presentation: CheckPresentation): CheckPresentationTone {
  return CHECK_PRESENTATION_VIEW_POLICY[presentation].tone;
}

export function CheckPresentationIcon({
  presentation,
  size,
}: {
  presentation: CheckPresentation;
  size: number;
}): ReactNode {
  return CHECK_PRESENTATION_VIEW_POLICY[presentation].renderIcon(size);
}
