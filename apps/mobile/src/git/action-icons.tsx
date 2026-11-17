import { withUnistyles } from "react-native-unistyles";
import {
  Archive,
  ArrowDownUp,
  Download,
  GitCommitHorizontal,
  GitMerge,
  RefreshCcw,
  Upload,
} from "lucide-react-native";
import { mutedIconColorMapping } from "@/components/ui/icon-color";

const ThemedGitCommitHorizontal = withUnistyles(GitCommitHorizontal);
const ThemedDownload = withUnistyles(Download);
const ThemedUpload = withUnistyles(Upload);
const ThemedArrowDownUp = withUnistyles(ArrowDownUp);
const ThemedGitMerge = withUnistyles(GitMerge);
const ThemedRefreshCcw = withUnistyles(RefreshCcw);
const ThemedArchive = withUnistyles(Archive);

export const GIT_ACTION_ICONS = {
  commit: <ThemedGitCommitHorizontal size={16} uniProps={mutedIconColorMapping} />,
  pull: <ThemedDownload size={16} uniProps={mutedIconColorMapping} />,
  push: <ThemedUpload size={16} uniProps={mutedIconColorMapping} />,
  pullAndPush: <ThemedArrowDownUp size={16} uniProps={mutedIconColorMapping} />,
  merge: <ThemedGitMerge size={16} uniProps={mutedIconColorMapping} />,
  resetWorkspace: <ThemedRefreshCcw size={16} uniProps={mutedIconColorMapping} />,
  archive: <ThemedArchive size={16} uniProps={mutedIconColorMapping} />,
};
