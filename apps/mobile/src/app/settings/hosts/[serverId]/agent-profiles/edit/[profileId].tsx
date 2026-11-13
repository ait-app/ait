import { useLocalSearchParams } from "expo-router";
import { useMemo } from "react";
import { HostRouteBootstrapBoundary } from "@/components/host-route-bootstrap-boundary";
import SettingsScreen from "@/screens/settings-screen";
import { normalizeProjectSettingsRouteId } from "@/utils/host-routes";

export default function SettingsHostAgentProfileEditRoute() {
  const params = useLocalSearchParams<{
    serverId?: string | string[];
    profileId?: string | string[];
  }>();
  const serverId = normalizeProjectSettingsRouteId(params.serverId);
  const profileId = normalizeProjectSettingsRouteId(params.profileId);
  // An id-less edit URL must report "not found", not fall back to the create form:
  // choosing "create" for a broken link would add a profile on Save.
  const view = useMemo(
    () => ({ kind: "agentProfile" as const, serverId, profileId }),
    [profileId, serverId],
  );

  return (
    <HostRouteBootstrapBoundary>
      <SettingsScreen view={view} />
    </HostRouteBootstrapBoundary>
  );
}
