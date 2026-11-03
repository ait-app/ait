import { HostRouteBootstrapBoundary } from "@/components/host-route-bootstrap-boundary";
import { SharedResourcesScreen } from "@/screens/shared-resources-screen";

export default function SharedRoute() {
  return (
    <HostRouteBootstrapBoundary>
      <SharedResourcesScreen />
    </HostRouteBootstrapBoundary>
  );
}
