import { useTranslation } from "react-i18next";
import { supportsBrowserAccount } from "@/runtime/browser-account";

export function ManagementSection() {
  const { t } = useTranslation();
  if (!supportsBrowserAccount()) return null;

  // The website owns management routes and checks permissions using the shared
  // same-origin session. Never pass credentials through the frame URL.
  return (
    <iframe
      src="/manage"
      title={t("settings.sections.management")}
      data-testid="management-console"
      style={{ display: "block", flex: 1, width: "100%", height: "100%", minHeight: 0, border: 0 }}
    />
  );
}
