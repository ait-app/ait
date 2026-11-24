import { withUnistyles } from "react-native-unistyles";
import {
  ArrowDownUp,
  Download,
  GitCommitHorizontal,
  GitMerge,
  RefreshCcw,
  SlidersHorizontal,
  Upload,
} from "lucide-react-native";
import { mutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedGitCommitHorizontal = withUnistyles(GitCommitHorizontal);
const ThemedDownload = withUnistyles(Download);
const ThemedUpload = withUnistyles(Upload);
const ThemedArrowDownUp = withUnistyles(ArrowDownUp);
const ThemedGitMerge = withUnistyles(GitMerge);
const ThemedRefreshCcw = withUnistyles(RefreshCcw);
const ThemedSlidersHorizontal = withUnistyles(SlidersHorizontal);

export const GIT_ACTION_ICONS = {
  commit: <ThemedGitCommitHorizontal size={16} uniProps={mutedIconColorMapping} />,
  pull: <ThemedDownload size={16} uniProps={mutedIconColorMapping} />,
  push: <ThemedUpload size={16} uniProps={mutedIconColorMapping} />,
  rebase: <ThemedArrowDownUp size={16} uniProps={mutedIconColorMapping} />,
  merge: <ThemedGitMerge size={16} uniProps={mutedIconColorMapping} />,
  resetWorkspace: <ThemedRefreshCcw size={16} uniProps={mutedIconColorMapping} />,
  mergeMethod: <ThemedSlidersHorizontal size={16} uniProps={mutedIconColorMapping} />,
};
