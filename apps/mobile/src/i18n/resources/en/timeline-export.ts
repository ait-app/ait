export const timelineExport = {
  title: "Export session",
  description:
    "Saved history only. Conversation includes user, assistant and voice transcripts. Timeline also includes saved cards. Maximum 16 MiB.",
  conversation: "Conversation only",
  timeline: "Whole saved Timeline",
  download: "Export and download",
  cancel: "Cancel export",
  unavailable: "Export unavailable. Connect to a host that supports timeline exports.",
  tooLarge: "Export exceeds the 16 MiB limit. Nothing was truncated.",
  browserHandoff: "Sent to browser — check your browser’s downloads to finish saving.",
};

export const nativeHtmlExport = {
  title: "Native HTML export (OMP)",
  description:
    "Oh My Pi renders this session with its own formatter. The file may contain active scripts and is saved as a download only — it is never opened inside Ait. Maximum 16 MiB.",
  download: "Export native HTML",
  risk: "The native format contains scripts and may load an external viewer (stencil.so) when opened; that viewer can read the session history embedded in the file. Download only — Ait never previews or opens it. Not the inert generic HTML export.",
  unavailable: "Native HTML export unavailable. Connect to an OMP host that supports it.",
};
