import type { ReactElement } from "react";
import { withUnistyles } from "react-native-unistyles";
import { SquareDot, SquareMinus, SquarePlus } from "lucide-react-native";
import { useTranslation } from "react-i18next";
import {
  statusDangerIconColorMapping,
  statusSuccessIconColorMapping,
  statusWarningIconColorMapping,
} from "@/components/ui/icon-color";

const ThemedSquarePlus = withUnistyles(SquarePlus);
const ThemedSquareMinus = withUnistyles(SquareMinus);
const ThemedSquareDot = withUnistyles(SquareDot);

const ICON_SIZE = 14;

/**
 * The file's change kind, shown beside its diff stat.
 *
 * The square variants form one aligned visual family for added, deleted, and modified files.
 */
export function FileChangeIcon({
  change,
}: {
  change: "added" | "deleted" | "modified";
}): ReactElement {
  const { t } = useTranslation();

  if (change === "added") {
    return (
      <ThemedSquarePlus
        size={ICON_SIZE}
        uniProps={statusSuccessIconColorMapping}
        accessibilityLabel={t("workspace.git.diff.newFile")}
      />
    );
  }

  if (change === "deleted") {
    return (
      <ThemedSquareMinus
        size={ICON_SIZE}
        uniProps={statusDangerIconColorMapping}
        accessibilityLabel={t("workspace.git.diff.deletedFile")}
      />
    );
  }

  return (
    <ThemedSquareDot
      size={ICON_SIZE}
      uniProps={statusWarningIconColorMapping}
      accessibilityLabel={t("workspace.git.diff.modifiedFile")}
    />
  );
}
