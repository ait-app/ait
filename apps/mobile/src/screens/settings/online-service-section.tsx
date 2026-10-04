import React from "react";
import { useTranslation } from "react-i18next";
import { AccountHostPanel } from "@/components/account-host-panel";
import { SettingsSection } from "@/components/settings";

export function OnlineServiceSection() {
  const { t } = useTranslation();
  return (
    <SettingsSection
      title={t("onlineService.accountTitle")}
      info={t("onlineService.loginDescription")}
      testID="settings-online-service"
    >
      <AccountHostPanel showHosts={false} />
    </SettingsSection>
  );
}
