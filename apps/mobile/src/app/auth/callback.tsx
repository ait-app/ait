import { Redirect } from "expo-router";

/** Native account authority consumes the URL event; routing never reads its credentials. */
export default function AccountCallback() {
  return <Redirect href="/settings/online-service" />;
}
