import { useEffect, useMemo, useSyncExternalStore } from "react";
import {
  openAgentProfileForm,
  type AgentProfileFormModel,
  type AgentProfileFormSnapshot,
  type AgentProfileFormState,
} from "./profile-form-model";

/**
 * One model per host + mode + profile identity. The page keys its body on
 * mode + profile id, and this hook additionally rebuilds the model when the
 * host changes underneath a still-mounted form: a late catalog from the new
 * host must never re-derive the old host's stored values, and a save must
 * never write one host's form state into another host's list.
 */
export function useAgentProfileFormModel(
  serverId: string,
  snapshot: AgentProfileFormSnapshot,
): AgentProfileFormModel {
  const profileId = snapshot.profile?.id ?? "";
  const model = useMemo(
    () => openAgentProfileForm(snapshot),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- identity-scoped: a new host or profile constructs a fresh model; other snapshot changes belong to the mounted form
    [serverId, snapshot.mode, profileId],
  );

  useEffect(() => {
    return () => {
      model.close();
    };
  }, [model]);

  return model;
}

export function useAgentProfileFormState(model: AgentProfileFormModel): AgentProfileFormState {
  return useSyncExternalStore(model.subscribe, model.getState, model.getState);
}
