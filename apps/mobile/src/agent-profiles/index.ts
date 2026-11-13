/**
 * Agent profiles: named bundles of composer settings (provider, model, mode,
 * thinking option, feature values, notes) stored host-wide in daemon config.
 *
 * Five capabilities leave this module — managing the list in settings, the
 * nested create/edit page that list navigates to, reading the list, pinning it
 * to the model picker, and drawing a profile's glyph. Everything else (the form
 * model, the catalog and feature probes, the materialization rules, the icon
 * registry, the row chrome) is internal; import from `@/agent-profiles`, never
 * a path inside it.
 *
 * `useAgentProfilePicker` deliberately hands the picker a flat row view model
 * and one `applyProfile(id)` callback rather than the profiles themselves: what
 * a profile contains, and how it reaches a live agent versus a draft, stays in
 * here.
 */
export type { AgentProfile } from "@ait/protocol/messages";
export type { MaterializedAgentProfile } from "./internal/materialize-profile";
export { useAgentProfiles } from "./internal/use-agent-profiles";
export {
  useAgentProfilePicker,
  type AgentProfileApplyTarget,
  type AgentProfilePicker,
  type AgentProfilePickerRow,
  type DraftAgentProfileControls,
} from "./internal/use-agent-profile-picker";
export { AgentProfileGlyph } from "./internal/agent-profile-glyph";
export {
  AgentProfileEditPage,
  type AgentProfileEditPageProps,
} from "./settings/agent-profile-edit-page";
export { AgentProfilesSection } from "./settings/agent-profiles-section";
