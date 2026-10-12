import { useEffect, useState } from "react";
import {
  openScheduleForm,
  type ScheduleFormSnapshot,
  type ScheduleFormAgent,
} from "./schedule-form-model";
const EMPTY_AGENTS: readonly ScheduleFormAgent[] = [];

export function useScheduleFormModel(snapshot: ScheduleFormSnapshot) {
  const [model] = useState(() => openScheduleForm(snapshot));

  useEffect(() => {
    return () => {
      model.close();
    };
  }, [model]);

  useEffect(() => {
    model.applyHosts(snapshot.hosts);
    model.applyProjectTargets(snapshot.defaults.projectTargets);
    model.applyPreferences(snapshot.defaults.preferences);
    model.applyAgents(snapshot.defaults.agents ?? EMPTY_AGENTS);
  }, [
    model,
    snapshot.hosts,
    snapshot.defaults.preferences,
    snapshot.defaults.projectTargets,
    snapshot.defaults.agents,
  ]);

  return model;
}
